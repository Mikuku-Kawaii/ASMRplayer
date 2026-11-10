import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type { SupabaseRequest } from "./supabaseSync";

export function listenForSupabaseSync() {
  return listen<{ id: string; request: SupabaseRequest }>("sync:supabase", async ({ payload }) => {
    let result;
    try {
      const { execute } = await import("./supabaseSync");
      result = { data: await execute(payload.request) };
    } catch (e) {
      result = { error: e instanceof Error ? e.message : "Supabase 请求失败" };
    }
    await invoke("supabase_sync_reply", { id: payload.id, result });
  }, { target: "main" });
}
