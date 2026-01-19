fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();

    if target.contains("windows") {
        // Tell cargo to rerun if these files change
        println!("cargo:rerun-if-changed=app.rc");
        println!("cargo:rerun-if-changed=app.manifest");
        println!("cargo:rerun-if-changed=assets/Icons/icon.ico");

        // On native Windows, use embed-resource (most reliable for MSVC)
        #[cfg(target_os = "windows")]
        {
            embed_resource::compile("app.rc", embed_resource::NONE);
            println!("cargo:warning=Embedded Windows resources using embed-resource");
            return;
        }

        // Cross-compilation from non-Windows: use mingw windres
        #[cfg(not(target_os = "windows"))]
        {
            let out_dir = std::env::var("OUT_DIR").unwrap();
            let res_path = format!("{}/app.res", out_dir);

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
                    println!("cargo:warning=Embedded Windows resources using windres");
                } else {
                    println!("cargo:warning=windres failed - Windows manifest will NOT be embedded!");
                }
            } else {
                println!("cargo:warning=windres not found - Windows manifest will NOT be embedded!");
            }
        }
    }
}
