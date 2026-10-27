//! The one HTTP agent builder. Every network caller states its policy here
//! (timeouts, https-only, status handling); nothing else builds a ureq agent.

use std::time::Duration;

/// A caller's limits. ureq's defaults apply to anything not listed.
#[derive(Clone, Copy)]
pub struct Policy {
    pub connect_secs: Option<u64>,
    pub global_secs: u64,
    pub https_only: bool,
    /// ureq's own default is true (4xx/5xx become errors); callers that read
    /// the status themselves turn it off.
    pub status_as_error: bool,
    /// `None` keeps ureq's default User-Agent.
    pub user_agent: Option<&'static str>,
}

/// AI chat stream: reads the status itself.
pub const AI: Policy = Policy {
    connect_secs: Some(10),
    global_secs: 300,
    https_only: false,
    status_as_error: false,
    user_agent: None,
};
/// OCR / translation model download.
pub const MODELS: Policy = Policy {
    connect_secs: Some(30),
    global_secs: 600,
    https_only: true,
    status_as_error: true,
    user_agent: None,
};
/// Release check and update download: nothing sent but the request and a User-Agent.
pub const UPDATE: Policy = Policy {
    connect_secs: Some(20),
    global_secs: 900,
    https_only: true,
    status_as_error: false,
    user_agent: Some("OpenCircle"),
};
/// Song identification upload.
pub const SONG: Policy = Policy {
    connect_secs: Some(10),
    global_secs: 30,
    https_only: false,
    status_as_error: false,
    user_agent: None,
};
/// Song cover image.
pub const COVER: Policy = Policy {
    connect_secs: None,
    global_secs: 10,
    https_only: false,
    status_as_error: true,
    user_agent: None,
};

pub fn agent(p: Policy) -> ureq::Agent {
    let mut b = ureq::Agent::config_builder()
        .https_only(p.https_only)
        .http_status_as_error(p.status_as_error)
        .timeout_connect(p.connect_secs.map(Duration::from_secs))
        .timeout_global(Some(Duration::from_secs(p.global_secs)));
    if let Some(ua) = p.user_agent {
        b = b.user_agent(ua);
    }
    b.build().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (policy, connect, global, https_only, status_as_error, custom UA): the
    /// values each caller had before the builders were merged.
    #[test]
    fn policies_match_the_old_builders() {
        let d = |s| Some(Duration::from_secs(s));
        let table = [
            (AI, d(10), d(300), false, false, false),
            (MODELS, d(30), d(600), true, true, false),
            (UPDATE, d(20), d(900), true, false, true),
            (SONG, d(10), d(30), false, false, false),
            (COVER, None, d(10), false, true, false),
        ];
        for (p, connect, global, https, status, ua) in table {
            let a = agent(p);
            let c = a.config();
            assert_eq!(c.timeouts().connect, connect);
            assert_eq!(c.timeouts().global, global);
            assert_eq!(c.https_only(), https);
            assert_eq!(c.http_status_as_error(), status);
            assert_eq!(p.user_agent.is_some(), ua);
        }
    }

    /// Only this file builds a ureq agent.
    #[test]
    fn one_http_agent() {
        fn walk(dir: &std::path::Path, hits: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, hits);
                } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("net.rs") {
                    let s = std::fs::read_to_string(&p).unwrap();
                    if s.contains("ureq::Agent::")
                        || s.contains("AgentBuilder")
                        || s.contains("config_builder")
                    {
                        hits.push(p.display().to_string());
                    }
                }
            }
        }
        let mut hits = vec![];
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut hits,
        );
        assert!(
            hits.is_empty(),
            "build agents with net::agent only: {hits:?}"
        );
    }
}
