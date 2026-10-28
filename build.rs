//! Embeds the app icon and version info in the Windows executable. The version
//! is Cargo's (`CARGO_PKG_VERSION`), which release.yml stamps from the git tag.
// ponytail: keyed on the host OS, so native Windows builds only (CI); cross builds skip it.
//
// The Settings previews: each `assets/previews/<clip>.webp` that exists is
// bundled (`$OUT_DIR/previews.rs`, `CLIPS`); a missing one is the
// placeholder (docs/previews.md).
//
// The setup build: `OPENCIRCLE_SETUP_MSI=<the release MSI>` embeds that MSI
// (`setup::payload`, cfg `setup_payload`) and the exe becomes OpenCircle-Setup.
/// The previews' clips, by name (`settings::previews`).
const CLIPS: [&str; 7] = ["circle", "text", "translate", "ai", "snip", "live", "song"];

fn previews() {
    // the folder: a clip added or removed reruns this
    println!("cargo::rerun-if-changed=assets/previews");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/previews");
    let mut out = String::from("pub const CLIPS: &[(&str, &[u8])] = &[\n");
    for name in CLIPS {
        let file = dir.join(format!("{name}.webp"));
        if file.is_file() {
            let path = file.to_str().expect("a UTF-8 path");
            out.push_str(&format!("    ({name:?}, include_bytes!({path:?})),\n"));
        }
    }
    out.push_str("];\n");
    let to = std::path::Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("previews.rs");
    std::fs::write(to, out).expect("write previews.rs");
}

fn main() {
    previews();
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
