<#
Run only an isolated test build with a separate Tauri identifier/profile.
Keep a paused current track in that profile and enable the desktop island to
exercise early WebView layout requests during native window initialization.
This probe starts and terminates only its own child process; it does not install
the application or alter an existing profile.
#>
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [Parameter(Mandatory = $true)][string]$ReportPath,
    [ValidateRange(5, 120)][int]$DurationSeconds = 20,
    [ValidateRange(5, 60)][int]$StartupTimeoutSeconds = 15
)
$ErrorActionPreference = 'Stop'
$binary = (Resolve-Path -LiteralPath $Executable).Path
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class StartupResponsiveness {
    public delegate bool Callback(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] static extern bool EnumWindows(Callback callback, IntPtr parameter);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] static extern bool IsHungAppWindow(IntPtr window);
    [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr window, uint message,
        IntPtr wParam, IntPtr lParam, uint flags, uint timeout, out IntPtr result);
    public static bool[] Check(uint process) {
        var results = new List<bool>();
        EnumWindows((window, parameter) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner == process && IsWindowVisible(window)) {
                IntPtr result;
                bool responds = SendMessageTimeout(window, 0, IntPtr.Zero, IntPtr.Zero,
                    2, 1000, out result) != IntPtr.Zero;
                results.Add(responds && !IsHungAppWindow(window));
            }
            return true;
        }, IntPtr.Zero);
        return results.ToArray();
    }
}
'@
$child = Start-Process -FilePath $binary -WorkingDirectory (Split-Path -Parent $binary) -WindowStyle Hidden -PassThru
$records = [System.Collections.Generic.List[object]]::new()
$clock = [System.Diagnostics.Stopwatch]::StartNew()
$firstVisibleAt = $null
try {
    do {
        Start-Sleep -Milliseconds 500
        $child.Refresh()
        if ($child.HasExited) { throw "Test process exited during startup: $($child.ExitCode)" }
        $responses = @([StartupResponsiveness]::Check($child.Id))
        $records.Add([pscustomobject]@{ elapsedMs = $clock.ElapsedMilliseconds; windowResponses = $responses })
        if ($responses.Count -gt 0) {
            if ($null -eq $firstVisibleAt) { $firstVisibleAt = $clock.Elapsed.TotalSeconds }
            if ($responses -contains $false) { throw 'A test window stopped responding to WM_NULL.' }
        } elseif ($null -ne $firstVisibleAt) {
            throw 'All test windows disappeared.'
        } elseif ($clock.Elapsed.TotalSeconds -gt $StartupTimeoutSeconds) {
            throw 'No visible test window appeared before the startup deadline.'
        }
    } while ($null -eq $firstVisibleAt -or $clock.Elapsed.TotalSeconds - $firstVisibleAt -lt $DurationSeconds)
    Write-Output "PASS: test windows remained responsive for $DurationSeconds seconds."
} finally {
    try {
        $records | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $ReportPath -Encoding utf8
    } finally {
        $child.Refresh()
        if (-not $child.HasExited) { Stop-Process -Id $child.Id -Force }
    }
}
