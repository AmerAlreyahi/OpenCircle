//! Embeds the app icon and version info in the Windows executable. The version
//! is Cargo's (`CARGO_PKG_VERSION`), which release.yml stamps from the git tag.
// ponytail: keyed on the host OS, so native Windows builds only (CI); cross builds skip it.
//
// The Settings previews: every `assets/previews/<name>.webp` found is bundled
// (`$OUT_DIR/previews.rs`, `CLIPS`); a page lists the names it wants and a
// missing one is simply absent (docs/previews.md).
//
// The setup build: `OPENCIRCLE_SETUP_MSI=<the release MSI>` embeds that MSI
// (`setup::payload`, cfg `setup_payload`) and the exe becomes OpenCircle-Setup.
fn previews() {
    // only the stills (the clips are downloaded on "Play preview"); the
    // folder: a still added or removed reruns this
    println!("cargo::rerun-if-changed=assets/previews/stills");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/previews/stills");
    let mut files: Vec<_> = (std::fs::read_dir(&dir).into_iter().flatten())
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "webp"))
        .collect();
    files.sort();
    let mut out = String::from("pub const STILLS: &[(&str, &[u8])] = &[\n");
    for file in files {
        let name = (file.file_stem().and_then(|n| n.to_str())).expect("a UTF-8 name");
        let path = file.to_str().expect("a UTF-8 path");
        out.push_str(&format!("    ({name:?}, include_bytes!({path:?})),\n"));
    }
    out.push_str("];\n");
    let to = std::path::Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("previews.rs");
    std::fs::write(to, out).expect("write previews.rs");
}

fn main() {
    previews();
    println!("cargo::rerun-if-changed=assets/icons/opencircle.ico");
    println!("cargo::rustc-check-cfg=cfg(setup_payload)");
    // The DLLs the exe imports load from System32 only, never from its own folder (the
    // setup runs elevated: a DLL planted beside it must not load, #151 review).
    // 0x800 = LOAD_LIBRARY_SEARCH_SYSTEM32.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo::rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    }
    println!("cargo::rerun-if-env-changed=OPENCIRCLE_SETUP_MSI");
    let setup = std::env::var_os("OPENCIRCLE_SETUP_MSI").map(|msi| {
        let msi = std::fs::canonicalize(&msi).expect("OPENCIRCLE_SETUP_MSI names a file");
        println!("cargo::rerun-if-changed={}", msi.display());
        println!("cargo::rustc-env=SETUP_MSI={}", msi.display());
        println!("cargo::rustc-cfg=setup_payload");
    });
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        let _ = setup;
        return;
    }
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
}
