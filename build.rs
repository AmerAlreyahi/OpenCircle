//! Embeds the app icon and version info in the Windows executable. The version
//! is Cargo's (`CARGO_PKG_VERSION`), which release.yml stamps from the git tag.
// ponytail: keyed on the host OS, so native Windows builds only (CI); cross builds skip it.
//
// The setup build: `OPENCIRCLE_SETUP_MSI=<the release MSI>` embeds that MSI
// (`setup::payload`, cfg `setup_payload`) and the exe becomes OpenCircle-Setup.
fn main() {
    println!("cargo::rerun-if-changed=assets/icons/opencircle.ico");
    println!("cargo::rustc-check-cfg=cfg(setup_payload)");
    println!("cargo::rerun-if-env-changed=OPENCIRCLE_SETUP_MSI");
    let setup = std::env::var_os("OPENCIRCLE_SETUP_MSI").map(|msi| {
        let msi = std::fs::canonicalize(&msi).expect("OPENCIRCLE_SETUP_MSI names a file");
        println!("cargo::rerun-if-changed={}", msi.display());
        println!("cargo::rustc-env=SETUP_MSI={}", msi.display());
        println!("cargo::rustc-cfg=setup_payload");
    });
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/icons/opencircle.ico")
        .set("ProductName", "OpenCircle")
        .set(
            "FileDescription",
            match setup {
                Some(()) => "OpenCircle Setup",
                None => "OpenCircle - circle anything on screen to search, read, translate or ask about it",
            },
        )
        .set(
            "OriginalFilename",
            match setup {
                Some(()) => "OpenCircle-Setup.exe",
                None => "opencircle.exe",
            },
        )
        .set("LegalCopyright", "MIT License")
        .compile()
        .expect("embed the Windows icon and version info");
    #[cfg(not(windows))]
    let _ = setup;
}
