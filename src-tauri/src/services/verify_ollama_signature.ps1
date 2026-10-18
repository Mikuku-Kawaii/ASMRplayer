# Exit codes are consumed by ollama_install.rs. Never execute the inspected file.
$ErrorActionPreference = 'Stop'
try {
    $signature = Get-AuthenticodeSignature -LiteralPath $env:ASMR_OLLAMA_INSTALLER
    switch ($signature.Status.ToString()) {
        'Valid' {
            if ($null -eq $signature.SignerCertificate -or $signature.SignerCertificate.Subject -notmatch '(?i)Ollama') { exit 20 }
            exit 0
        }
        'NotSigned' { exit 21 }
        'HashMismatch' { exit 22 }
        'NotTrusted' { exit 23 }
        'NotSupportedFileFormat' { exit 24 }
        'Incompatible' { exit 25 }
        default { exit 26 }
    }
} catch {
    exit 27
}
