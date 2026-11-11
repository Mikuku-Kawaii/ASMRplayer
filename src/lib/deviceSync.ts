import { useCallback, useEffect, useRef, useState } from "react";
import { callCommand, normalizeAppError } from "./api";
export type SyncProvider = "github" | "onedrive" | "webdav" | "supabase";
export interface SyncResume { id: string; versionId: string; title: string; positionMs: number; updatedMs: number; deviceName: string }
export interface SyncDetails { summary: { kind: string; label: string; total: number; received: number; pending: number }[];
  pendingItems: { key: string; label: string; kind: string; reason: string }[] }
export interface DeviceSyncStatus { provider: SyncProvider; configured: boolean; connected: boolean; records: number; pending: number;
  lastSync: string | null; autoError?: string; metadataPending?: number; metadataError?: string; phase?: string;
  details?: SyncDetails; resume?: SyncResume | null;
  autoPolicy: { foreground: boolean; periodic: boolean; intervalMinutes: number };
  conflicts: { key: string; label: string; choices: { device: string; seq: number; value: unknown }[] }[] }
export const syncRequest = (action: string, input: unknown = null, provider?: SyncProvider) =>
  callCommand<DeviceSyncStatus>("device_sync_command", { action, input, provider });
export function useDeviceSyncStatus() {
  const [status, setStatus] = useState<DeviceSyncStatus>();
  const [error, setError] = useState("");
  const running = useRef(false), mounted = useRef(false);
  const refresh = useCallback(async () => {
    if (running.current || document.hidden) return;
    running.current = true;
    try { const value = await syncRequest("status"); if (mounted.current) { setStatus(value); setError(""); } }
    catch(e) { if (mounted.current) setError(normalizeAppError(e).message); }
    finally { running.current = false; }
  }, []);
  useEffect(() => {
    mounted.current = true; void refresh();
    const timer = window.setInterval(() => void refresh(), 4000);
    const visible = () => { if (!document.hidden) void refresh(); };
    document.addEventListener("visibilitychange", visible);
    return () => { mounted.current = false; window.clearInterval(timer); document.removeEventListener("visibilitychange",visible); };
  }, [refresh]);
  return { status, error, refresh };
}
