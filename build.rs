//! Embeds the app icon and version info in the Windows executable. The version
//! is Cargo's (`CARGO_PKG_VERSION`), which release.yml stamps from the git tag.
// ponytail: keyed on the host OS, so native Windows builds only (CI); cross builds skip it.
fn main() {
    println!("cargo::rerun-if-changed=assets/icons/opencircle.ico");
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/icons/opencircle.ico")
        .set("ProductName", "OpenCircle")
        .set(
            "FileDescription",
            "OpenCircle - circle anything on screen to search, read, translate or ask about it",
        )
        .set("OriginalFilename", "opencircle.exe")
        .set("LegalCopyright", "MIT License")
        .compile()
        .expect("embed the Windows icon and version info");
}
