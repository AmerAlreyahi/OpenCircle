//! AudD (docs.audd.io): POST the clip as multipart/form-data with the user's
//! `api_token`; the answer is JSON: `{"status":"success","result":{...}|null}`
//! or `{"status":"error","error":{"error_code":N,"error_message":"..."}}`.

use serde_json::Value;

use super::{Clip, Error, Song, youtube};
use crate::secrets::Secret;

const URL: &str = "https://api.audd.io/";
/// Extra metadata asked for: Apple Music has the cover, Spotify the track link.
const RETURN: &str = "apple_music,spotify";
// ponytail: a fixed boundary; a 30-char random-looking run inside 240 KB of PCM
// is not a real risk. Random per request if a service ever chokes on it.
const BOUNDARY: &str = "opencircle-song-7d3f9a1c5e2b8046";
/// The cover's size (Apple's image server makes any size, and PNG on request).
const COVER: &str = "300x300bb.png";

/// The request body and its content type.
fn request(key: &Secret, clip: &Clip) -> (String, Vec<u8>) {
    let mut body = Vec::new();
    for (name, value) in [("api_token", key.expose()), ("return", RETURN)] {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"clip.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&clip.wav());
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={BOUNDARY}"), body)
}

pub fn identify(key: &Secret, clip: &Clip) -> Result<Song, Error> {
    let agent = crate::net::agent(crate::net::SONG);
    let (kind, body) = request(key, clip);
    let mut resp = agent
        .post(URL)
        .header("Content-Type", kind)
        .send(&body[..])
        .map_err(|_| Error::Offline)?;
    let text = resp
        .body_mut()
        .with_config()
        .limit(1 << 20)
        .read_to_string()
        .map_err(|_| Error::Offline)?;
    parse(&text).map_err(|e| match e {
        Error::Service(why) => Error::Service(crate::ai::scrub(&why, Some(key))),
        e => e,
    })
}

fn parse(body: &str) -> Result<Song, Error> {
    let v: Value = serde_json::from_str(body)
        .map_err(|_| Error::Service("unexpected answer from the service".into()))?;
    let s = |v: &Value, key: &str| v[key].as_str().unwrap_or_default().trim().to_string();
    if v["status"] == "error" {
        let e = &v["error"];
        return Err(match e["error_code"].as_i64() {
            // 900 = invalid token; 901 = no token and the anonymous limit is used up.
            Some(900 | 901) => Error::BadKey,
            _ => Error::Service(match s(e, "error_message") {
                m if m.is_empty() => "the service refused the request".into(),
                m => m,
            }),
        });
    }
    let r = &v["result"];
    if v["status"] != "success" || !r.is_object() || s(r, "title").is_empty() {
        return Err(Error::NotFound);
    }
    let year = s(r, "release_date").chars().take(4).collect::<String>();
    let mut song = Song {
        title: s(r, "title"),
        artist: s(r, "artist"),
        album: s(r, "album"),
        year: if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
            year
        } else {
            String::new()
        },
        cover: cover(
            r["apple_music"]["artwork"]["url"]
                .as_str()
                .unwrap_or_default(),
        ),
        links: Vec::new(),
    };
    let https = |u: String| u.starts_with("https://").then_some(u);
    let spotify = https(s(&r["spotify"]["external_urls"], "spotify"));
    let page = https(s(r, "song_link"));
    song.links.extend(spotify.map(|u| ("Spotify", u)));
    song.links
        .push(("YouTube", youtube(&song.artist, &song.title)));
    song.links.extend(page.map(|u| ("More", u)));
    Ok(song)
}

/// Apple's artwork template (`.../{w}x{h}bb.jpg`) as a PNG at our size; only
/// from Apple's image server.
fn cover(template: &str) -> Option<String> {
    let (base, _) = template.split_once("{w}x{h}")?;
    let host = crate::sys::open::web_host(base)?;
    (base.starts_with("https://") && host.ends_with(".mzstatic.com"))
        .then(|| format!("{base}{COVER}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOUND: &str = include_str!("fixtures/audd_found.json");

    #[test]
    fn a_match_becomes_a_song_with_cover_and_links() {
        let song = parse(FOUND).unwrap();
        assert_eq!(song.title, "Wallpaper");
        assert_eq!(song.artist, "Kevin MacLeod");
        assert_eq!(song.album, "Royalty Free");
        assert_eq!(song.year, "2026");
        assert_eq!(song.detail(), "Royalty Free · 2026");
        assert_eq!(
            song.cover.as_deref(),
            Some(
                "https://is1-ssl.mzstatic.com/image/thumb/Music211/v4/48/8d/18/488d1881-6542-7156-e7f2-47d416711694/13716.jpg/300x300bb.png"
            )
        );
        let labels: Vec<_> = song.links.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, ["Spotify", "YouTube", "More"]);
        assert_eq!(
            song.links[0].1,
            "https://open.spotify.com/track/1ZBF2NMfnncz9HL0YZJieo"
        );
        assert_eq!(song.links[2].1, "https://lis.tn/mxzrXE");
        for (_, url) in &song.links {
            assert!(crate::sys::open::web_host(url).is_some(), "{url}");
        }
    }

    #[test]
    fn no_match_and_errors_map_to_what_the_user_sees() {
        assert_eq!(
            parse(include_str!("fixtures/audd_none.json")),
            Err(Error::NotFound)
        );
        assert_eq!(
            parse(include_str!("fixtures/audd_error_901.json")),
            Err(Error::BadKey)
        );
        let invalid =
            r#"{"status":"error","error":{"error_code":900,"error_message":"Invalid token"}}"#;
        assert_eq!(parse(invalid), Err(Error::BadKey));
        let other = r#"{"status":"error","error":{"error_code":300,"error_message":"Fingerprinting error"}}"#;
        assert_eq!(
            parse(other),
            Err(Error::Service("Fingerprinting error".into()))
        );
        assert!(matches!(parse("<html>502</html>"), Err(Error::Service(_))));
        assert_eq!(
            parse(r#"{"status":"success","result":[]}"#),
            Err(Error::NotFound)
        );
    }

    #[test]
    fn only_https_links_and_apples_covers_are_kept() {
        let body = r#"{"status":"success","result":{"title":"T","artist":"A","release_date":"n/a",
            "song_link":"javascript:alert(1)",
            "spotify":{"external_urls":{"spotify":"http://open.spotify.com/x"}},
            "apple_music":{"artwork":{"url":"https://evil.example/{w}x{h}bb.jpg"}}}}"#;
        let song = parse(body).unwrap();
        assert_eq!(song.year, "");
        assert_eq!(song.cover, None);
        let labels: Vec<_> = song.links.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, ["YouTube"]);
        assert_eq!(cover("https://a.mzstatic.com/x.jpg"), None, "no template");
    }

    #[test]
    fn the_request_is_multipart_with_token_return_and_wav() {
        let key = Secret::new("FAKE-key-123");
        let (kind, body) = request(&key, &Clip(vec![7; 4]));
        assert_eq!(kind, format!("multipart/form-data; boundary={BOUNDARY}"));
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with(&format!("--{BOUNDARY}\r\n")));
        assert!(text.contains("name=\"api_token\"\r\n\r\nFAKE-key-123\r\n"));
        assert!(text.contains("name=\"return\"\r\n\r\napple_music,spotify\r\n"));
        assert!(text.contains(
            "name=\"file\"; filename=\"clip.wav\"\r\nContent-Type: audio/wav\r\n\r\nRIFF"
        ));
        assert!(text.ends_with(&format!("\r\n--{BOUNDARY}--\r\n")));
        // Only the token, the field list and the clip: nothing else about the user.
        assert_eq!(text.matches("Content-Disposition").count(), 3);
        let wav = Clip(vec![7; 4]).wav();
        assert!(
            body.windows(wav.len()).any(|w| w == wav),
            "the clip, byte for byte"
        );
    }

    /// Real call; needs your own key in AUDD_TEST_KEY and a WAV (12 kHz mono) in AUDD_TEST_WAV.
    #[test]
    #[ignore]
    fn live_audd_lookup() {
        let (Ok(key), Ok(wav)) = (
            std::env::var("AUDD_TEST_KEY"),
            std::env::var("AUDD_TEST_WAV"),
        ) else {
            return;
        };
        let bytes = std::fs::read(wav).unwrap();
        let samples = bytes[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c))
            .collect();
        let song = identify(&Secret::new(key), &Clip(samples)).unwrap();
        crate::log!("{song:?}");
        assert!(!song.title.is_empty());
    }
}
