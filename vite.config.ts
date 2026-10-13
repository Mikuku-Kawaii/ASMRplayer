import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 14520,
    strictPort: true,
    fs: {
      deny: [".env", ".env.*", "*.{crt,pem}", "**/.git/**", "**/.release-signing/**"],
    },
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 14521,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**", "**/.tmp/**", "**/output/**", "**/.release-signing/**"],
    },
  },
});
