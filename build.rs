fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();

    if target.contains("windows") {
        // Try to use windres from mingw for cross-compilation
        let out_dir = std::env::var("OUT_DIR").unwrap();
        let res_path = format!("{}/app.res", out_dir);

        // Try mingw windres first (for cross-compilation)
        let windres_cmd = if target.contains("x86_64") {
            "x86_64-w64-mingw32-windres"
        } else {
            "i686-w64-mingw32-windres"
        };

        let status = std::process::Command::new(windres_cmd)
            .args(["app.rc", "-O", "coff", "-o", &res_path])
            .status();

        if let Ok(status) = status {
            if status.success() {
                println!("cargo:rustc-link-arg={}", res_path);
                println!("cargo:rerun-if-changed=app.rc");
                println!("cargo:rerun-if-changed=app.manifest");
                return;
            }
        }

        // Fallback: try embed-resource (works on native Windows)
        #[cfg(target_os = "windows")]
        {
            embed_resource::compile("app.rc", embed_resource::NONE);
        }
    }
}
