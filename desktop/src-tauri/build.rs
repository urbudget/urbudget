fn main() {
    tauri_build::build();
    // Tauri embeds its Windows manifest in binaries, not integration tests or QA examples.
    // Both mock and real runtimes link TaskDialogIndirect, which needs Common Controls v6.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        for target in ["tests", "examples"] {
            println!("cargo:rustc-link-arg-{target}=/MANIFEST:EMBED");
            println!("cargo:rustc-link-arg-{target}=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
        }
    }
}
