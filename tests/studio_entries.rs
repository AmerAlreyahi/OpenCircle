//! OpenCircle Studio's packaging entries (Start menu, "Open with", desktop file): the name is the
//! string table's English `st_title`, the entries are extra choices and never a default.
// Windows checks text files out with CRLF: compare with LF.
fn read_to_string(p: &str) -> std::io::Result<String> {
    std::fs::read_to_string(p).map(|s| s.replace("\r\n", "\n"))
}

fn st_title() -> String {
    let t = read_to_string("src/shared/app_language/strings.rs").unwrap();
    let rest = &t[t
        .find("st_title: \"")
        .expect("st_title in the string table")
        + 11..];
    rest[..rest.find('"').unwrap()].to_string()
}

#[test]
fn the_desktop_file_parses_and_names_the_studio() {
    let d = read_to_string("packaging/com.opencircle.app.studio.desktop").unwrap();
    let line = |k: &str| {
        d.lines()
            .find_map(|l| l.strip_prefix(k))
            .unwrap_or_else(|| panic!("{k} missing"))
            .to_string()
    };
    assert_eq!(d.lines().next(), Some("[Desktop Entry]"));
    assert_eq!(line("Name="), st_title());
    assert_eq!(line("Exec="), "opencircle studio %f");
    assert_eq!(
        line("MimeType="),
        "image/png;image/jpeg;image/webp;image/bmp;image/gif;"
    );
    assert!(!d.contains("NoDisplay"));
}

#[test]
fn deb_and_rpm_ship_the_studio_desktop_file_but_the_appimage_does_not() {
    let c = read_to_string("Cargo.toml").unwrap();
    let dest = "applications/com.opencircle.app.studio.desktop";
    // deb and rpm only. The AppImage must NOT: linuxdeploy links every desktop file into the
    // AppDir root and AppRun runs the first one (the Studio's, `studio %f`) with the user's
    // arguments, so `--version` / `run` / `capture` / `quit` broke in 0.5.0 (W126).
    assert_eq!(c.matches(dest).count(), 2, "deb + rpm");
    let a = &c[c.find("[package.metadata.packager.appimage]").unwrap()..];
    let a = &a[..a.find("[package.metadata.generate-rpm]").unwrap()];
    assert!(
        !a.contains("studio.desktop\""),
        "the AppImage ships no Studio entry"
    );
}

#[test]
fn the_wxs_has_the_start_menu_shortcut_and_open_with_entries() {
    let w = read_to_string("assets/installer/main.wxs").unwrap();
    let name = st_title();
    assert!(w.contains(&format!(
        "Name=\"{name}\"\n                    Description=\"Edit"
    )));
    assert!(w.contains("Arguments=\"studio\""));
    assert!(w.contains(&format!(
        "FriendlyAppName\" Type=\"string\" Value=\"{name}\""
    )));
    assert!(w.contains("Classes\\Applications\\opencircle.exe"));
    assert!(w.contains("studio &quot;%1&quot;"));
    for ext in [".png", ".jpg", ".jpeg", ".webp", ".bmp", ".gif"] {
        assert!(
            w.contains(&format!("Name=\"{ext}\" Type=\"string\" Value=\"\"")),
            "{ext} supported"
        );
        assert!(
            w.contains(&format!("Classes\\{ext}\\OpenWithProgids")),
            "{ext} progid"
        );
    }
    // No default association: no extension key gets a (Default) value or a UserChoice.
    assert!(!w.contains("UserChoice"));
    assert!(!w.contains("Root=\"HKLM\" Key=\"Software\\Classes\\.png\">"));
    assert!(w.contains("<ComponentRef Id=\"StudioOpenWith\"/>"));
}

#[test]
fn install_sh_ships_and_removes_the_studio_entry() {
    let s = read_to_string("scripts/install.sh").unwrap();
    let d = read_to_string("packaging/com.opencircle.app.studio.desktop").unwrap();
    assert!(s.contains("$APP_ID.studio.desktop"));
    assert!(s.contains("Exec=$BIN studio %f"));
    for k in ["Name=", "Comment=", "Categories=", "MimeType="] {
        let l = d.lines().find(|l| l.starts_with(k)).unwrap();
        assert!(s.contains(l), "{l} differs from the packaged file");
    }
    assert!(
        s.contains("\"$STUDIO\" \"$AUTOSTART\"; do"),
        "uninstall removes it"
    );
    assert!(!s.contains("mimeapps"), "never touches the defaults");
}
