import { build } from "vite";
await build({
  configFile: false,
  publicDir: false,
  build: {
    outDir: process.argv[2] ?? "android/app/build/generated/supabaseAssets",
    target: "chrome85",
    lib: { entry: "src/lib/supabaseSync.ts", name: "AsmrSync", formats: ["iife"], fileName: () => "supabase-sync.js" },
  },
});
