//! Every folder OpenCircle keeps data in, named once. The on-disk names are the ones
//! v0.1.x already wrote, letter case included: `OpenCircle/` (logs, models, search
//! history, the old web view) and `opencircle/` (config, song history, recent captures,
//! updates). They are NOT merged: an installed copy finds its files where it left them.

use std::path::{Path, PathBuf};

const CAPITAL: &str = "OpenCircle";
const LOWER: &str = "opencircle";

/// `<data>/OpenCircle` (the temp folder when the OS has no data folder).
pub fn app_data() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(CAPITAL)
}

/// `<data>/OpenCircle/logs`.
pub fn logs() -> PathBuf {
    app_data().join("logs")
}

/// `<data>/OpenCircle/mt`: translation models.
pub fn mt() -> PathBuf {
    app_data().join("mt")
}

/// `<data>/OpenCircle/ocr`: ORT and the OCR models.
pub fn ocr() -> PathBuf {
    app_data().join("ocr")
}

/// `<data>/OpenCircle/search_history.json`.
pub fn search_history() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(CAPITAL).join("search_history.json"))
}

fn lower() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(LOWER))
}

/// `<data>/opencircle/song_history.json`.
pub fn song_history() -> Option<PathBuf> {
    lower().map(|d| d.join("song_history.json"))
}

/// `<data>/opencircle/recent`: recent captures.
pub fn recent() -> Option<PathBuf> {
    lower().map(|d| d.join("recent"))
}

/// `<data>/opencircle/opencircle.db`: the app's one database (`store`).
pub fn database() -> Option<PathBuf> {
    lower().map(|d| d.join("opencircle.db"))
}

/// `<data>/opencircle/update`: where a download is staged.
pub fn update_stage() -> Option<PathBuf> {
    lower().map(|d| d.join("update"))
}

/// `<data>/opencircle/update.json`: the marker an update leaves for the next start.
pub fn update_marker() -> Option<PathBuf> {
    lower().map(|d| d.join("update.json"))
}

/// `<config>/opencircle/config.toml`.
pub fn config_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(LOWER).join("config.toml"))
}

/// Everything the app keeps for the user: the settings folder and both spellings of
/// the data folder (what "forget my data" removes).
pub fn user_data() -> Vec<PathBuf> {
    user_data_in(
        dirs::data_local_dir().as_deref(),
        dirs::config_dir().as_deref(),
    )
}

/// `user_data` over these roots (`None` = the OS has none).
pub fn user_data_in(data: Option<&Path>, config: Option<&Path>) -> Vec<PathBuf> {
    let mut v: Vec<_> = config.map(|c| c.join(LOWER)).into_iter().collect();
    if let Some(d) = data {
        v.extend([d.join(LOWER), d.join(CAPITAL)]);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Today's names, relative to the data / config root: the table is the contract
    /// and changes only with a migration.
    #[test]
    fn every_folder_keeps_the_name_it_has_on_disk() {
        let rel = |p: PathBuf, root: PathBuf| {
            p.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        };
        let data = dirs::data_local_dir().unwrap();
        for (got, want) in [
            (app_data(), "OpenCircle"),
            (logs(), "OpenCircle/logs"),
            (mt(), "OpenCircle/mt"),
            (ocr(), "OpenCircle/ocr"),
            (search_history().unwrap(), "OpenCircle/search_history.json"),
            (song_history().unwrap(), "opencircle/song_history.json"),
            (recent().unwrap(), "opencircle/recent"),
            (database().unwrap(), "opencircle/opencircle.db"),
            (update_stage().unwrap(), "opencircle/update"),
            (update_marker().unwrap(), "opencircle/update.json"),
        ] {
            assert_eq!(rel(got, data.clone()), want);
        }
        let cfg = dirs::config_dir().unwrap();
        assert_eq!(rel(config_file().unwrap(), cfg), "opencircle/config.toml");
    }

    #[test]
    fn user_data_is_the_config_folder_and_both_data_spellings() {
        let (d, c) = (Path::new("/d"), Path::new("/c"));
        assert_eq!(
            user_data_in(Some(d), Some(c)),
            [
                c.join("opencircle"),
                d.join("opencircle"),
                d.join("OpenCircle")
            ]
        );
        assert!(user_data_in(None, None).is_empty());
    }
}
