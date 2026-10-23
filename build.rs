//! Embeds the app icon in the Windows executable.
// ponytail: keyed on the host OS, so native Windows builds only (CI); cross builds skip it.
fn main() {
    println!("cargo::rerun-if-changed=assets/icons/opencircle.ico");
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/icons/opencircle.ico")
        .compile()
        .expect("embed the Windows icon");
}
