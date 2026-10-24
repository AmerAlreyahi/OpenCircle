//! Is a download really ours? A minisign signature over `SHA256SUMS.txt` (made by
//! the release workflow with the private key that lives only in a GitHub Actions
//! secret), then the asset's SHA-256 from that file. No valid signature, no install.

use std::io::Read;

/// The release signing key (minisign public key, the base64 line of `minisign.pub`).
/// Empty = no key yet: every update is refused (fail closed).
// The lead replaces this with the real key; its private half is the MINISIGN_KEY
// secret of the release workflow (never in the repository).
pub const RELEASE_PUBLIC_KEY: &str = "RWSbS2U2Cjix7vMbkgnr+jEZI2VKl7+HWo2Scq4uImBxKNgXCPo+K5oY";

/// `sums` is signed by `key` (base64 public key) with signature file `sig`, and the
/// signed (trusted) comment is `OpenCircle v<tag_version>`: an old signed release
/// replayed under a newer tag is refused.
pub fn signed(sums: &[u8], sig: &str, key: &str, tag_version: &str) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("this build has no release key".into());
    }
    let key = minisign_verify::PublicKey::from_base64(key.trim())
        .map_err(|e| format!("bad release key: {e}"))?;
    let sig =
        minisign_verify::Signature::decode(sig).map_err(|e| format!("bad signature file: {e}"))?;
    key.verify(sums, &sig, false)
        .map_err(|e| format!("signature does not match: {e}"))?;
    let want = format!("OpenCircle v{tag_version}");
    if sig.trusted_comment() != want {
        return Err(format!(
            "signature is for \"{}\", not \"{want}\"",
            sig.trusted_comment()
        ));
    }
    Ok(())
}

/// The SHA-256 that `sums` (`sha256sum` output: `<hex>  <name>` or `<hex> *<name>`)
/// lists for `name`.
pub fn sum_for(sums: &str, name: &str) -> Option<[u8; 32]> {
    sums.lines().find_map(|line| {
        let (hex, file) = line.split_once(' ')?;
        let file = file.trim_start_matches([' ', '*']);
        (file == name).then(|| parse_hex(hex)).flatten()
    })
}

fn parse_hex(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// SHA-256 of everything `r` yields.
pub fn sha256(mut r: impl Read) -> std::io::Result<[u8; 32]> {
    let mut hash = hmac_sha256::Hash::new();
    let mut buf = vec![0; 64 * 1024];
    loop {
        match r.read(&mut buf)? {
            0 => return Ok(hash.finalize()),
            n => hash.update(&buf[..n]),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A TEST keypair made for these tests only (its secret key was never committed).
    pub const TEST_KEY: &str = "RWSgzdo6hR+6Sc4BS7E5EbUQ0oNvTzpWOxUcHtjz7NCgdU/drq8dH4wL";
    pub const WRONG_KEY: &str = "RWTHvR4v6FeTQDjOyNbkFag6emGYS/XU+pGF0h0Vc3mCNWeDwcebDBLc";
    pub const EXE: &[u8] = b"OpenCircle 9.9.9 test build\n";
    pub const APPIMAGE: &[u8] = b"OpenCircle 9.9.9 test appimage\n";
    pub const SUMS: &str = "\
4072929c317ec620b528e2c89db29a59d3d48950a68abf2050ef988c3aa4a7e3  OpenCircle_9.9.9_x64-portable.exe
7648ecaf62b6aa287953bbc20590685263e9e3328f2ac4cd04018c294fedd78a  OpenCircle_9.9.9_x86_64.AppImage
";
    /// `SUMS` signed by `TEST_KEY`.
    pub const SIG: &str = "untrusted comment: signature from rsign secret key
RUSgzdo6hR+6SXDqXJshXB6Uye/0C7TrTkRpJp4Su7PYFs/wGEjIkkoJwnn9wNL9de2XcwrOSug54JDoFWuXHg6iKCE6LuK7dQQ=
trusted comment: OpenCircle v9.9.9
WCpy6e+eWE3H34iMPtayBwO3eavwLD+Y7kqziwOB9ppmNXBwF16cko/XSKgB8qGBMThWkl84UTCkXhHE4P5yDQ==
";

    /// `SUMS` signed by `TEST_KEY` with rsign's default comment (none of ours).
    const NO_COMMENT_SIG: &str = "untrusted comment: signature from rsign secret key
RUSgzdo6hR+6Sfr38VpeDWtNhwFrBTsn6Ou4H5h6ZL9LISum6gePqKK9ieZzsMdnzU+zeYXwiT5weKk30wpiKc+DO0BjBwg4iwo=
trusted comment: timestamp:1792867461	file:SUMS.txt	prehashed
bCZq7D3cwe5DAF0R6TSagJIqwC0BZO+vusSZAFC6bxzBa8ibUFgdu9kHymlpIuimT93TuzkI1b5jcg7Svly0Bg==
";

    #[test]
    fn a_good_signature_passes() {
        assert_eq!(signed(SUMS.as_bytes(), SIG, TEST_KEY, "9.9.9"), Ok(()));
    }

    #[test]
    fn the_trusted_comment_must_name_the_release() {
        // Replay: a genuine signature of another version (or a bare one) is refused.
        assert!(signed(SUMS.as_bytes(), SIG, TEST_KEY, "9.9.8").is_err());
        assert!(signed(SUMS.as_bytes(), SIG, TEST_KEY, "").is_err());
        assert!(signed(SUMS.as_bytes(), NO_COMMENT_SIG, TEST_KEY, "9.9.9").is_err());
    }

    #[test]
    fn tampered_sums_wrong_key_missing_key_or_garbage_fail() {
        let tampered = SUMS.replace("4072929c", "4072929d");
        assert!(signed(tampered.as_bytes(), SIG, TEST_KEY, "9.9.9").is_err());
        assert!(signed(SUMS.as_bytes(), SIG, WRONG_KEY, "9.9.9").is_err());
        assert!(
            signed(SUMS.as_bytes(), SIG, "", "9.9.9").is_err(),
            "no key = fail closed"
        );
        assert!(signed(SUMS.as_bytes(), "", TEST_KEY, "9.9.9").is_err());
        assert!(signed(SUMS.as_bytes(), "not a signature", TEST_KEY, "9.9.9").is_err());
    }

    #[test]
    fn the_shipped_key_is_empty_or_valid() {
        if !RELEASE_PUBLIC_KEY.is_empty() {
            assert!(minisign_verify::PublicKey::from_base64(RELEASE_PUBLIC_KEY).is_ok());
        }
    }

    #[test]
    fn sums_are_found_by_exact_name_and_match_the_bytes() {
        let want = sha256(EXE).unwrap();
        assert_eq!(
            sum_for(SUMS, "OpenCircle_9.9.9_x64-portable.exe"),
            Some(want)
        );
        assert_eq!(
            sum_for(
                &SUMS.replace("  ", " *"),
                "OpenCircle_9.9.9_x64-portable.exe"
            ),
            Some(want)
        );
        assert_eq!(sum_for(SUMS, "x64-portable.exe"), None);
        assert_eq!(sum_for(SUMS, "missing"), None);
        assert_ne!(
            sum_for(SUMS, "OpenCircle_9.9.9_x86_64.AppImage"),
            Some(want)
        );
        assert_eq!(sum_for("zz  a", "a"), None);
    }
}
