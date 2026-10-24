//! Ask AI: a chat about the crop with the provider the user configured
//! (`crate::ai`). Nothing is sent until the user asks a question.
use std::fmt;
use std::panic::{self, AssertUnwindSafe};

use super::{Action, Crop, Key, Panel, Run};
use crate::ai::{self, Role};
use crate::config::{AiConfig, Config, Provider};
use crate::secrets::{self, Secret};

pub const ACTION: Action = Action {
    id: "ai",
    label: "Ask AI",
    tip: "Ask an AI about the selection",
    key: Key {
        ctrl: false,
        letter: 'a',
    },
    icon: '\u{e416}', // lucide "sparkles"
    run: Run::Panel(Panel::Ask),
};

const SYSTEM: &str = "The image is part of the user's screen that they selected. \
Answer their questions about it briefly and plainly, in the language they write in. \
Use plain text, no Markdown.";

/// What a chat needs: the provider settings, its key and the crop as PNG.
pub struct Setup {
    pub cfg: AiConfig,
    key: Option<Secret>,
    pub png: Vec<u8>,
}

impl fmt::Debug for Setup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Setup")
            .field("cfg", &self.cfg)
            .finish_non_exhaustive()
    }
}

impl Setup {
    /// The no-key provider: questions open in Google AI Mode in the browser.
    pub fn browser(&self) -> bool {
        self.cfg.provider == Provider::Browser
    }
}

/// Reads the config and the key, encodes the crop. Blocking (file, keyring,
/// PNG): call off the UI thread. `Err` = what to tell the user, with the steps.
pub fn setup(crop: &Crop) -> Result<Setup, String> {
    setup_for(crate::config::current().ai, crop)
}

fn setup_for(cfg: AiConfig, crop: &Crop) -> Result<Setup, String> {
    if cfg.provider == Provider::Browser {
        // No key, and the image never leaves (Lens re-encodes the crop itself).
        return Ok(Setup {
            cfg,
            key: None,
            png: Vec::new(),
        });
    }
    let (cfg, key) = credentials_for(cfg)?;
    let png = super::encode_png(crop.width, crop.height, &crop.rgba)?;
    Ok(Setup { cfg, key, png })
}

/// The provider settings and the key, checked. Blocking (file, keyring): call
/// off the UI thread. `Err` = what to tell the user (first line), then the steps.
pub fn credentials() -> Result<(AiConfig, Option<Secret>), String> {
    credentials_for(crate::config::current().ai)
}

/// The one gate every AI use passes (Ask AI, Explain, translate): an unknown
/// provider stops here, before the keyring, the network or the browser.
fn credentials_for(cfg: AiConfig) -> Result<(AiConfig, Option<Secret>), String> {
    let why = match &cfg.provider {
        Provider::Invalid(raw) => {
            format!("Your AI provider setting \"{raw}\" isn't recognised. Fix it in Settings > AI.")
        }
        Provider::Unreadable => "Your settings file couldn't be read; AI is off until you \
            fix it in Settings > AI."
            .into(),
        _ => String::new(),
    };
    if !why.is_empty() {
        return Err(format!("{why}\n\nNothing was sent anywhere."));
    }
    if cfg.provider == Provider::Browser {
        return Err(
            "This needs an AI provider with an API key: Google AI Mode in your \
            browser cannot send text back.\n\nIn Settings > AI choose Anthropic or \
            OpenAI-compatible and add your key."
                .into(),
        );
    }
    let key = match secrets::get(cfg.provider.clone()) {
        Ok(key) => key.filter(|key| !key.is_empty()),
        // A local OpenAI-compatible server needs no key: go on without the keyring.
        Err(e) if cfg.provider == Provider::OpenaiCompatible => {
            eprintln!("keyring: {e}");
            None
        }
        Err(e) => return Err(format!("{e}\n\n{}", steps(&cfg.provider))),
    };
    let missing = match cfg.provider {
        Provider::Anthropic if key.is_none() => Some("no API key is saved"),
        Provider::OpenaiCompatible if cfg.base_url.trim().is_empty() => {
            Some("no endpoint (base_url) is set")
        }
        _ if cfg.model.trim().is_empty() => Some("no model is set"),
        _ => None,
    };
    if let Some(missing) = missing {
        return Err(format!(
            "AI is not set up yet: {missing}.\n\n{}",
            steps(&cfg.provider)
        ));
    }
    Ok((cfg, key))
}

/// The exact steps to set the AI up by hand (until the Settings window does it).
fn steps(provider: &Provider) -> String {
    let path = Config::path().map_or("the OpenCircle config file".into(), |p| {
        p.display().to_string()
    });
    let account = format!("ai-key-{}", provider.id());
    #[cfg(target_os = "linux")]
    let keyring = format!(
        "In a terminal: secret-tool store --label=OpenCircle service opencircle username {account}\n\
         then paste the key."
    );
    #[cfg(windows)]
    let keyring = format!(
        "Credential Manager > Windows Credentials > Add a generic credential:\n\
         address {account}.opencircle, user name {account}, password = the key."
    );
    #[cfg(not(any(target_os = "linux", windows)))]
    let keyring = format!("Service \"opencircle\", account \"{account}\".");
    format!(
        "1. In {path} add:\n\
         [ai]\n\
         provider = \"anthropic\"  (or \"openai_compatible\")\n\
         model = \"{}\"\n\
         base_url = \"\"  (openai_compatible: e.g. http://localhost:11434/v1)\n\n\
         2. Save the API key in the system keyring, never in that file.\n{keyring}\n\n\
         Then restart OpenCircle and press A again.\n\n\
         Easier: tray icon > Settings > AI does all of this and needs no restart.",
        ai::DEFAULT_ANTHROPIC_MODEL
    )
}

/// One question and its answer so far.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub question: String,
    pub answer: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Status {
    #[default]
    Idle,
    Streaming,
    Done,
    Failed(String),
    /// Stopped by the user; the partial answer stays.
    Cancelled,
}

/// The conversation in the Ask AI panel.
#[derive(Debug, Default)]
pub struct Chat {
    pub turns: Vec<Turn>,
    pub status: Status,
}

impl Chat {
    /// Starts a turn and returns the conversation to send (the crop goes with the
    /// first question). `None` while an answer streams, or for a blank question.
    pub fn ask(&mut self, question: &str, png: &[u8]) -> Option<Vec<ai::Message>> {
        let question = question.trim();
        if question.is_empty() || self.status == Status::Streaming {
            return None;
        }
        // Earlier turns without an answer (failed at once) are shown, not sent.
        let mut messages = vec![ai::Message::new(Role::System, SYSTEM)];
        for turn in self.turns.iter().filter(|t| !t.answer.is_empty()) {
            messages.push(ai::Message::user(&turn.question));
            messages.push(ai::Message::new(Role::Assistant, &turn.answer));
        }
        messages.push(ai::Message::user(question));
        messages[1].image_png = Some(png.to_vec());
        self.turns.push(Turn {
            question: question.into(),
            answer: String::new(),
        });
        self.status = Status::Streaming;
        Some(messages)
    }

    pub fn delta(&mut self, piece: &str) {
        if let (Status::Streaming, Some(turn)) = (&self.status, self.turns.last_mut()) {
            turn.answer.push_str(piece);
        }
    }

    pub fn finish(&mut self, result: Result<(), String>) {
        if self.status == Status::Streaming {
            self.status = result.map_or_else(Status::Failed, |()| Status::Done);
        }
    }

    pub fn cancel(&mut self) {
        if self.status == Status::Streaming {
            self.status = Status::Cancelled;
        }
    }
}

/// Unwinds out of `ai::ask_stream` when the reader is gone.
struct Stopped;

/// Streams the answer to `messages`; `on_delta` gets each piece and returns
/// `false` to stop. Blocking (network): call off the UI thread.
///
/// The browser provider calls no API: the last message (the question, already
/// holding any text read from the selection) opens in Google AI Mode in the
/// user's browser, and `on_delta` gets one line saying so.
pub fn stream(
    setup: &Setup,
    messages: &[ai::Message],
    mut on_delta: impl FnMut(&str) -> bool,
) -> Result<(), String> {
    if setup.browser() {
        let question = messages.last().map_or("", |m| m.text.as_str());
        on_delta(&ai::browser::ask(question)?);
        return Ok(());
    }
    let run = panic::catch_unwind(AssertUnwindSafe(|| {
        ai::ask_stream(&setup.cfg, setup.key.as_ref(), messages, |piece| {
            if !on_delta(piece) {
                // ponytail: ask_stream has no way to stop; unwinding out of the
                // callback drops the connection. Let on_delta return "stop" when
                // src/ai/ is next changed. resume_unwind skips the panic hook.
                panic::resume_unwind(Box::new(Stopped));
            }
        })
    }));
    match run {
        Ok(answer) => answer.map(drop).map_err(|e| e.to_string()),
        Err(payload) if payload.is::<Stopped>() => Err("Stopped.".into()),
        Err(payload) => panic::resume_unwind(payload),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    #[test]
    fn chat_goes_idle_streaming_done() {
        let mut chat = Chat::default();
        assert_eq!(chat.status, Status::Idle);
        assert!(chat.ask("   ", b"png").is_none(), "blank question");
        let sent = chat.ask(" What is this? ", b"png").unwrap();
        assert_eq!(chat.status, Status::Streaming);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].role, Role::System);
        assert_eq!(sent[1].text, "What is this?");
        assert_eq!(sent[1].image_png.as_deref(), Some(&b"png"[..]));
        assert!(chat.ask("again", b"png").is_none(), "one answer at a time");
        chat.delta("A tram");
        chat.delta(" in Lisbon.");
        chat.finish(Ok(()));
        assert_eq!(chat.status, Status::Done);
        assert_eq!(chat.turns[0].answer, "A tram in Lisbon.");
        chat.delta("late piece");
        assert_eq!(chat.turns[0].answer, "A tram in Lisbon.");

        // A follow-up resends the history; the image stays on the first question only.
        let sent = chat.ask("Where does it go?", b"png").unwrap();
        let texts: Vec<_> = sent.iter().map(|m| (m.role, m.text.as_str())).collect();
        assert_eq!(
            texts[1..],
            [
                (Role::User, "What is this?"),
                (Role::Assistant, "A tram in Lisbon."),
                (Role::User, "Where does it go?"),
            ]
        );
        assert!(sent[3].image_png.is_none());
    }

    #[test]
    fn chat_records_errors_and_cancels() {
        let mut chat = Chat::default();
        chat.ask("Explain", b"png").unwrap();
        chat.finish(Err("HTTP 401".into()));
        assert_eq!(chat.status, Status::Failed("HTTP 401".into()));
        // A failed turn with no answer is not resent.
        let sent = chat.ask("Summarize", b"png").unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].text, "Summarize");
        assert!(sent[1].image_png.is_some());
        chat.delta("partial");
        chat.cancel();
        assert_eq!(chat.status, Status::Cancelled);
        chat.finish(Ok(())); // the stopped stream's end changes nothing
        assert_eq!(chat.status, Status::Cancelled);
        assert_eq!(chat.turns[1].answer, "partial");
        chat.cancel();
        assert_eq!(chat.status, Status::Cancelled);
        assert!(chat.ask("Next", b"png").is_some(), "can ask again");
    }

    /// A one-shot OpenAI-compatible server on 127.0.0.1 that streams `pieces`.
    fn stub(pieces: &'static [&'static str]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
            }
            reader
                .by_ref()
                .take(length)
                .read_to_end(&mut Vec::new())
                .unwrap();
            let mut out = reader.into_inner();
            let _ = out.write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
            );
            for piece in pieces {
                let chunk = serde_json::json!({"choices": [{"delta": {"content": piece}}]});
                let _ = write!(out, "data: {chunk}\n\n");
                let _ = out.flush();
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let _ = out.write_all(b"data: [DONE]\n\n");
        });
        url
    }

    fn local_setup(base_url: String) -> Setup {
        Setup {
            cfg: AiConfig {
                provider: Provider::OpenaiCompatible,
                base_url,
                model: "stub".into(),
            },
            key: None,
            png: b"png".to_vec(),
        }
    }

    /// Runs the browser provider end to end with a fake browser (a script that
    /// records its one argument): the URL, and no call to any API server.
    #[cfg(all(unix, debug_assertions))]
    #[test]
    fn browser_provider_opens_ai_mode_and_calls_no_api() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("oc-ask-browser-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("fake-browser.sh");
        let seen = dir.join("url.txt");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s' \"$1\" > '{0}.tmp' && mv '{0}.tmp' '{0}'\n",
                seen.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        // SAFETY: the only test that touches this variable.
        unsafe { std::env::set_var("OPENCIRCLE_TEST_BROWSER", &script) };

        // A server that would answer, if the API client were used.
        let api = TcpListener::bind("127.0.0.1:0").unwrap();
        api.set_nonblocking(true).unwrap();
        let setup = Setup {
            cfg: AiConfig {
                provider: Provider::Browser,
                base_url: format!("http://{}/v1", api.local_addr().unwrap()),
                model: "stub".into(),
            },
            key: None,
            png: Vec::new(),
        };
        assert!(setup.browser());
        let question = ai::browser::question("Explain", Some("Pastel de nata\n1,40 €"));
        let mut lines = Vec::new();
        let result = stream(&setup, &[ai::Message::user(question)], |piece| {
            lines.push(piece.to_string());
            true
        });
        assert_eq!(result, Ok(()));
        assert_eq!(lines, ["Opened in your browser."]);

        let mut url = String::new();
        for _ in 0..100 {
            if let Ok(u) = std::fs::read_to_string(&seen) {
                url = u;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        println!("SAMPLE URL: {url}");
        assert_eq!(
            url,
            "https://www.google.com/search?udm=50&q=Explain%0A%0AText%20from%20my%20screen%3A%0A\
             Pastel%20de%20nata%0A1%2C40%20%E2%82%AC"
        );
        assert!(
            matches!(api.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "the API server was contacted"
        );

        // An unknown provider ("antropic") or a broken config file, with the same
        // fake browser and server: no URL opened, no HTTP request, both through
        // the real gate (setup) and even if a Setup were built by hand.
        std::fs::remove_file(&seen).unwrap();
        let broken = Config::parse("[ai]\nprovider = \"anthropic\"\nmodel = = =\n").0;
        for (provider, why) in [
            (Provider::Invalid("antropic".into()), "\"antropic\""),
            (broken.ai.provider, "couldn't be read"),
        ] {
            let cfg = AiConfig {
                provider,
                ..setup.cfg.clone()
            };
            let crop = Crop {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
            };
            assert!(setup_for(cfg.clone(), &crop).unwrap_err().contains(why));
            let invalid = Setup {
                cfg,
                key: None,
                png: b"png".to_vec(),
            };
            assert!(!invalid.browser());
            let mut got = Vec::new();
            let result = stream(&invalid, &[ai::Message::user("hi")], |piece| {
                got.push(piece.to_string());
                true
            });
            assert!(result.is_err() && got.is_empty(), "{result:?} {got:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!seen.exists(), "the browser was opened");
        assert!(
            matches!(api.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "the API server was contacted"
        );

        // A launcher that cannot start is reported, not swallowed.
        unsafe { std::env::set_var("OPENCIRCLE_TEST_BROWSER", dir.join("missing")) };
        let result = stream(&setup, &[ai::Message::user("hi")], |_| true);
        assert!(
            result
                .unwrap_err()
                .starts_with("Could not open the browser")
        );
        unsafe { std::env::remove_var("OPENCIRCLE_TEST_BROWSER") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The message Ask AI, Explain and translate show for an unknown provider.
    pub(crate) fn invalid_provider_error() -> String {
        let cfg = AiConfig {
            provider: Provider::Invalid("antropic".into()),
            ..AiConfig::default()
        };
        let crop = Crop {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        };
        setup_for(cfg, &crop).unwrap_err()
    }

    #[test]
    fn unknown_provider_stops_before_keyring_network_or_browser() {
        let msg = invalid_provider_error();
        assert!(
            msg.starts_with(
                "Your AI provider setting \"antropic\" isn't recognised. Fix it in Settings > AI."
            ),
            "{msg}"
        );
        // translate (`actions::translate::deps`) goes through the same gate.
        let cfg = AiConfig {
            provider: Provider::Invalid("antropic".into()),
            ..AiConfig::default()
        };
        assert_eq!(credentials_for(cfg).unwrap_err(), msg);
        // A config file that is there but broken: AI off, not Browser.
        let broken = Config::parse("theme = = =").0.ai;
        assert!(credentials_for(broken).unwrap_err().starts_with(
            "Your settings file couldn't be read; AI is off until you fix it in Settings > AI."
        ));
        // A known provider is unchanged: Browser still needs no key and sends no image.
        let crop = Crop {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        };
        let browser = setup_for(AiConfig::default(), &crop).unwrap();
        assert!(browser.browser() && browser.png.is_empty());
    }

    #[test]
    fn streams_from_a_local_server_and_stops_on_request() {
        let setup = local_setup(stub(&["Hello", ", ", "world"]));
        let mut chat = Chat::default();
        let messages = chat.ask("Explain", &setup.png).unwrap();
        let result = stream(&setup, &messages, |piece| {
            chat.delta(piece);
            true
        });
        chat.finish(result);
        assert_eq!(chat.status, Status::Done);
        assert_eq!(chat.turns[0].answer, "Hello, world");

        let setup = local_setup(stub(&["one", "two", "three"]));
        let mut got = Vec::new();
        let result = stream(&setup, &messages, |piece| {
            got.push(piece.to_string());
            false // the panel is gone
        });
        assert_eq!(result, Err("Stopped.".into()));
        assert_eq!(got, ["one"]);
    }
}
