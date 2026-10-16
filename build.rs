fn main() {
    println!("cargo:rerun-if-changed=assets/windows.manifest");
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winres::WindowsResource::new()
            .set_icon("assets/app-icon.ico")
            .set("ProductName", "墨转 Word")
            .compile()
            .expect("failed to embed Windows application icon");
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("assets/windows.manifest");
        println!("cargo:rustc-link-arg-bin=md-to-word=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=md-to-word=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
