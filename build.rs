fn main() {
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=LIBRARY_PATH");
    println!("cargo:rerun-if-env-changed=LD_LIBRARY_PATH");

    // 1. Try resolving gbm via pkg-config (standard for Linux/Wayland/Mesa)
    if pkg_config::probe_library("gbm").is_ok() {
        return;
    }

    // 2. Fallback: inspect search paths in environment for libgbm.so
    for env_var in &["LIBRARY_PATH", "LD_LIBRARY_PATH"] {
        if let Ok(val) = std::env::var(env_var) {
            for path in std::env::split_paths(&val) {
                if path.join("libgbm.so").exists() || path.join("libgbm.so.1").exists() {
                    println!("cargo:rustc-link-search=native={}", path.display());
                    println!("cargo:rustc-link-lib=gbm");
                    return;
                }
            }
        }
    }
}
