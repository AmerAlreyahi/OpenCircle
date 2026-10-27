//! release.yml uploads what the frozen update channel and the owner's asset list
//! expect: the MSI is built for the setup exe to carry (owner, 2026-10-27) but is
//! no release asset; the portable exe keeps both names (0.1.0's updater wants the old one).
const WORKFLOW: &str = include_str!("../.github/workflows/release.yml");

#[test]
fn the_msi_is_built_but_not_uploaded() {
    let upload = WORKFLOW
        .split("actions/upload-artifact")
        .nth(1)
        .expect("the build job uploads its files");
    let paths = upload.split("if-no-files-found").next().unwrap();
    assert!(
        !paths.contains("msi"),
        "the MSI must not be uploaded:\n{paths}"
    );
    for kept in ["dist/*.exe", "dist/*.AppImage", "dist/*.deb", "dist/*.rpm"] {
        assert!(paths.contains(kept), "{kept} is still uploaded");
    }
    assert!(
        WORKFLOW.contains(r#"OPENCIRCLE_SETUP_MSI="msi/"#),
        "the setup still embeds it"
    );
    assert!(
        !WORKFLOW.contains(r#"mv "$msi" "dist/"#),
        "the MSI stays out of dist/"
    );
    for name in [
        "OpenCircle-${version}-x64-portable.exe",
        "OpenCircle_${version}_x64-portable.exe",
    ] {
        assert!(WORKFLOW.contains(name), "{name}");
    }
}
