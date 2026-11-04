//! The full clips (owner, 2026-11-04): 1080p AV1, downloaded the first time
//! the user clicks "Play preview" on a page (nothing before that click), from a
//! GitHub release of the owner's, each pinned by size and SHA-256 like the
//! models (`ocr::models::ensure`: https only, at most the pinned size read, a
//! partial or wrong file deleted). Kept in `<data>/OpenCircle/previews`, so a
//! later visit plays from disk with no network. Made by `render-av1.py`
//! (docs/previews.md); a new clip = one line here + its still.
use std::path::PathBuf;

use crate::ocr::models::{self, ModelFile};

macro_rules! clip {
    ($name:literal, $size:expr, $sha256:literal) => {
        ModelFile::plain(
            concat!($name, ".ivf"),
            $size,
            $sha256,
            concat!(
                "https://github.com/AmerAlreyahi/OpenCircle-models/releases/download/previews-1/",
                $name,
                ".ivf"
            ),
        )
    };
}

/// Every clip the app may download, by file name.
pub(crate) const CLIPS: [ModelFile; 13] = [
    clip!(
        "circle-1",
        561_423,
        "2e23d1e3ea54fdd206cb0de8634f07cc356dfa5f4c80164e8393de9f0ac07d39"
    ),
    clip!(
        "circle-2",
        548_321,
        "0e8868cd3a49772a36222bf7d19a6ab333fba2501089dc35ceb835e6f5d4586c"
    ),
    clip!(
        "circle-3",
        568_020,
        "0a664a1fef1209a490e6299e582c365cb0e5ad158b1e39b767f6de920dd1a9ee"
    ),
    clip!(
        "text",
        914_104,
        "a5b77239a9f65dbc8d6b349d4a6be08d675053c1164cab99bb5cec7158eb1a6d"
    ),
    clip!(
        "ai-chat",
        418_566,
        "c3133e69922e939d77cd415fecb7c51d474d7c7a67d7658d0a258a3a66bc7a77"
    ),
    clip!(
        "ai-browser",
        471_752,
        "a74660c1ac3b68500b379f9aa268745ac3261cd4a9c8a7726114196920879d44"
    ),
    clip!(
        "snip-blur",
        168_607,
        "d93cad0c2d730a2aa6813f69421475dfd51152411a5b5bb61c7d14c9fe785197"
    ),
    clip!(
        "snip-comment",
        278_058,
        "6b0cfc083126f79e6e7e2e3826b9ab53813fbc4901d0e8997fb376619e30b637"
    ),
    clip!(
        "snip-long",
        652_219,
        "754451810c4b8a7a6200cbf6cd1bd51158312fd86248b731e7a820935c0476fd"
    ),
    clip!(
        "translate-1",
        540_851,
        "94a51634b4d108b4b094a35251d852bcdc0916bafc8ca0e3c93603f9a0c61594"
    ),
    clip!(
        "translate-2",
        521_310,
        "796ecd3097be9e29277a31d0e60401d93edbaa8b4ececf06900d89af325feebe"
    ),
    clip!(
        "translate-3",
        1_313_809,
        "9770e0645014cd88c55cf9b41d71a97d7d66c5ad695ba6b1a421af1dfefcbc1d"
    ),
    clip!(
        "translate-4",
        1_710_992,
        "23674dc14621c8b30e3297bee4a423502adc237eeaf6782558f1d978d054102d"
    ),
];

/// The clips `names` on disk, where they go (`None`: one isn't pinned).
pub fn files(names: &[&str]) -> Option<Vec<(PathBuf, &'static ModelFile)>> {
    files_in(&crate::paths::previews(), names)
}

pub(crate) fn files_in(
    dir: &std::path::Path,
    names: &[&str],
) -> Option<Vec<(PathBuf, &'static ModelFile)>> {
    (names.iter())
        .map(|n| {
            let f = CLIPS
                .iter()
                .find(|f| f.name.strip_suffix(".ivf") == Some(*n))?;
            Some((dir.join(f.name), f))
        })
        .collect()
}

/// All of them are on disk (sizes only: `load` checks each before use).
pub fn ready(files: &[(PathBuf, &ModelFile)]) -> bool {
    models::missing_bytes(files) == 0
}

/// Downloads the ones missing or damaged (blocking: a worker thread).
pub fn fetch(files: &[(PathBuf, &'static ModelFile)]) -> Result<(), String> {
    models::ensure(files, |_, _| {}).map_err(|e| {
        crate::log!("[opencircle] preview download failed: {e}");
        e.to_string()
    })
}

/// A clip's bytes, only when they are exactly the pinned file; a wrong one is
/// deleted (the next "Play preview" fetches it again).
pub fn load(path: &std::path::Path, f: &ModelFile) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    let hash: String = (hmac_sha256::Hash::hash(&bytes).iter())
        .map(|b| format!("{b:02x}"))
        .collect();
    if bytes.len() as u64 == f.size && hash == f.sha256 {
        return Some(bytes);
    }
    crate::log!("[opencircle] preview {} damaged: deleted", f.name);
    let _ = std::fs::remove_file(path);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::models::LOCAL_FAKE;

    /// "hello", pinned.
    const HELLO: ModelFile = ModelFile::plain(
        "hello.ivf",
        5,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        "",
    );

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oc-previews-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A clip is used only when it is exactly the pinned file; a damaged one
    /// is deleted (so "Play preview" fetches it again).
    #[test]
    fn a_clip_is_checked_before_use() {
        let d = dir("load");
        let path = d.join(HELLO.name);
        std::fs::write(&path, b"hello").unwrap();
        assert_eq!(load(&path, &HELLO).as_deref(), Some(b"hello".as_slice()));
        std::fs::write(&path, b"hellO").unwrap();
        assert!(ready(&[(path.clone(), &HELLO)]), "sizes only");
        assert!(load(&path, &HELLO).is_none());
        assert!(!path.exists(), "deleted");
        assert!(!ready(&[(path, &HELLO)]));
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A verified clip is never fetched again; a missing one that can't be
    /// fetched is a failure with nothing left behind (no partial file).
    #[test]
    fn a_download_verifies_and_leaves_no_partial_file() {
        LOCAL_FAKE.set(true); // no real network: HELLO's URL is empty
        let d = dir("fetch");
        let path = d.join(HELLO.name);
        std::fs::write(&path, b"hello").unwrap();
        assert!(
            fetch(&[(path.clone(), &HELLO)]).is_ok(),
            "on disk: nothing to do"
        );
        std::fs::remove_file(&path).unwrap();
        assert!(fetch(&[(path.clone(), &HELLO)]).is_err());
        assert!(!path.exists() && !path.with_extension("part").exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Only pinned clips, under the previews folder, over https from one release.
    #[test]
    fn the_clips_are_pinned_https_files() {
        let d = std::path::Path::new("/x");
        let f = files_in(d, &["snip-blur", "snip-long"]).unwrap();
        assert_eq!(f[0].0, d.join("snip-blur.ivf"));
        assert!(files_in(d, &["translate"]).is_none(), "held: not pinned");
        assert!(crate::paths::previews().ends_with("OpenCircle/previews"));
        let release =
            "https://github.com/AmerAlreyahi/OpenCircle-models/releases/download/previews-1/";
        for c in &CLIPS {
            assert!(
                format!("{c:?}").contains(&format!("{release}{}", c.name)),
                "{c:?}"
            );
        }
    }
}
