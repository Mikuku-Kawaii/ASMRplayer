import { createClient } from "@supabase/supabase-js";
import project from "../../config/supabase.json";

export type SupabaseRequest = {
  action: "login" | "signup" | "refresh" | "select" | "upsert";
  email?: string; password?: string; refreshToken?: string;
  session?: { accessToken: string; userId: string };
  after?: string; device?: string; document?: unknown;
};

// Native secure storage owns the refresh token. Each call runs in the same
// Supabase project on both platforms, including background syncs.
export async function execute(request: SupabaseRequest) {
  const { action, session } = request;
  const client = createClient(project.url, project.publishableKey, {
    auth: { persistSession: false, detectSessionInUrl: false, autoRefreshToken: false },
    ...(session && { accessToken: async () => session.accessToken }),
  });
  if (action === "select" || action === "upsert") {
    if (!session) throw new Error("请先登录 Supabase");
    const table = client.from("user_sync");
    if (action === "upsert") {
      const { error } = await table.upsert({
        user_id: session.userId, device_id: request.device, sync_data: request.document,
      }, { onConflict: "user_id,device_id" });
      if (error) throw new Error(error.message);
      return {};
    }
    let query = table.select("device_id,sync_data,updated_at").eq("user_id", session.userId);
    if (request.after) query = query.gt("device_id", request.after);
    const { data, error } = await query.order("device_id").limit(1).maybeSingle();
    if (error) throw new Error(error.message);
    return data ?? {};
  }
  const credentials = { email: request.email?.trim() ?? "", password: request.password ?? "" };
  const { data, error } = action === "refresh"
    ? await client.auth.refreshSession({ refresh_token: request.refreshToken ?? "" })
    : action === "signup" ? await client.auth.signUp(credentials)
    : await client.auth.signInWithPassword(credentials);
  if (error) throw new Error(error.message);
  if (!data.session) throw new Error(action === "signup" ? "请查看邮箱中的确认链接，确认后登录。" : "登录已过期，请重新登录。");
  return {
    refreshToken: data.session.refresh_token,
    accessToken: data.session.access_token,
    userId: data.session.user.id,
  };
}
