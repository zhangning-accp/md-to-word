fn main() {
    println!("cargo:rerun-if-changed=assets/windows.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("assets/windows.manifest");
        println!("cargo:rustc-link-arg-bin=md-to-word=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=md-to-word=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
