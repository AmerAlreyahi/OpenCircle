//! The user's language and region as the OS reports them, as BCP-47 tags
//! ("en-US", "pt-BR"): only letters, digits and '-', so a tag is safe in a
//! page or a file.

/// The region format's tag (Windows' user locale, else `LC_ALL` /
/// `LC_MESSAGES` / `LANG`), or "en": what Google's pages take as `hl`.
pub fn tag() -> String {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserDefaultLocaleName(buf: *mut u16, len: i32) -> i32;
        }
        let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
        // SAFETY: buf is valid for 85 u16 and we pass its length.
        let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
        if n > 1 {
            return clean(&String::from_utf16_lossy(&buf[..n as usize - 1]));
        }
        "en".into()
    }
    #[cfg(not(windows))]
    {
        clean(&env(&["LC_ALL", "LC_MESSAGES", "LANG"]))
    }
}

/// The language the user reads: Windows' first display language (Settings >
/// Language), else `LANGUAGE`'s first entry, else `tag`; "en" when there is
/// none.
pub fn language() -> String {
    #[cfg(windows)]
    {
        use windows::System::UserProfile::GlobalizationPreferences;
        let first = GlobalizationPreferences::Languages()
            .ok()
            .and_then(|v| v.into_iter().next())
            .map(|t| t.to_string());
        first.map_or_else(tag, |t| clean(&t))
    }
    #[cfg(not(windows))]
    {
        let languages = env(&["LANGUAGE"]);
        match languages.split(':').next().unwrap_or_default() {
            "" => tag(),
            first => clean(first),
        }
    }
}

/// The first of `keys` that is set and not empty.
#[cfg(not(windows))]
fn env(keys: &[&str]) -> String {
    (keys.iter())
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default()
}

/// "en_US.UTF-8" -> "en-US". Anything odd (C, POSIX, markup, too long) -> "en".
fn clean(raw: &str) -> String {
    let base = raw.split(['.', '@']).next().unwrap_or("").replace('_', "-");
    let ok = base.len() <= 16
        && base.starts_with(|c: char| c.is_ascii_alphabetic())
        && base.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !matches!(base.as_str(), "C" | "POSIX");
    if ok { base } else { "en".into() }
}

#[test]
fn tags_are_sanitised() {
    assert_eq!(clean("en_US.UTF-8"), "en-US");
    assert_eq!(clean("ar_SA@x"), "ar-SA");
    assert_eq!(clean("C"), "en");
    assert_eq!(clean("POSIX"), "en");
    assert_eq!(clean(""), "en");
    assert_eq!(clean("en\"><script>alert(1)</script>"), "en");
    assert_eq!(clean("-x"), "en");
}
