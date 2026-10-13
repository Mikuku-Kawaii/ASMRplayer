fn main() {
    // Copy vulkan-1.dll alongside the compiled binary in target/{profile} if present,
    // so the dynamically loaded libmpv runtime finds its companion DLL during dev runs.
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        let src_dll = std::path::Path::new(&manifest_dir)
            .join("binaries")
            .join("vulkan-1.dll");
        if src_dll.exists() {
            // OUT_DIR is typically target/{debug|release}/build/asmr-player-.../out
            let out_path = std::path::Path::new(&out_dir);
            if let Some(profile_dir) = out_path.ancestors().nth(3) {
                let dst_dll = profile_dir.join("vulkan-1.dll");
                let _ = std::fs::copy(&src_dll, &dst_dll);
            }
        }
    }
    tauri_build::build()
}
