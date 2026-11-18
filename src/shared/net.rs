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
/// OCR / translation model download: reads the status itself (a 5xx or 429
/// is retried, with its Retry-After).
pub const MODELS: Policy = Policy {
    connect_secs: Some(30),
    global_secs: 600,
    https_only: true,
    status_as_error: false,
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

/// Web search (Ask AI's `web_search`): the results pages of the search
/// engines, with an honest name (never a disguised browser).
pub const WEB: Policy = Policy {
    connect_secs: Some(10),
    global_secs: 15,
    https_only: true,
    status_as_error: false,
    user_agent: Some("OpenCircle (desktop app; web search)"),
};
/// A page the AI opens (`open_page`): public hosts only (`public_agent`).
pub const PAGE: Policy = Policy {
    connect_secs: Some(10),
    global_secs: 20,
    https_only: false,
    status_as_error: false,
    user_agent: Some("OpenCircle (desktop app; web search)"),
};

pub fn agent(p: Policy) -> ureq::Agent {
    config(p).into()
}

/// Like `agent`, but every address a host resolves to (redirects included)
/// must be public: never this PC, the local network or link-local (SSRF).
/// The check runs on the addresses actually connected to, so a name that
/// later resolves elsewhere (DNS rebinding) is refused too.
pub fn public_agent(p: Policy) -> ureq::Agent {
    ureq::Agent::with_parts(
        config(p),
        ureq::unversioned::transport::DefaultConnector::default(),
        PublicOnly,
    )
}

#[derive(Debug)]
struct PublicOnly;

impl ureq::unversioned::resolver::Resolver for PublicOnly {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        use ureq::unversioned::resolver::DefaultResolver;
        let all = DefaultResolver::default().resolve(uri, config, timeout)?;
        let mut out = self.empty();
        for a in all.iter().filter(|a| public(a.ip())) {
            out.push(*a);
        }
        match out.is_empty() {
            true => Err(ureq::Error::Io(std::io::Error::other(
                "not a public internet address",
            ))),
            false => Ok(out),
        }
    }
}

/// A public internet address: not loopback, private, link-local, shared
/// (100.64/10), documentation, broadcast or unspecified.
pub fn public(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || a == 0
                || (a == 100 && (64..128).contains(&b)))
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => public(IpAddr::V4(v4)),
            None => {
                !(v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_unique_local()
                    || v6.is_unicast_link_local())
            }
        },
    }
}

fn config(p: Policy) -> ureq::config::Config {
    let mut b = ureq::Agent::config_builder()
        .https_only(p.https_only)
        .http_status_as_error(p.status_as_error)
        .timeout_connect(p.connect_secs.map(Duration::from_secs))
        .timeout_global(Some(Duration::from_secs(p.global_secs)));
    if let Some(ua) = p.user_agent {
        b = b.user_agent(ua);
    }
    b.build()
}

/// The connection broke before an answer came back: reset, aborted, closed
/// early (EOF) or a broken pipe. Nothing was received, so the request may be
/// sent again on a new connection without repeating anything. Not "no
/// connection" (DNS, refused, unreachable): that stays an error at once.
pub fn dropped(e: &ureq::Error) -> bool {
    use std::io::ErrorKind as K;
    matches!(e, ureq::Error::Io(io) if matches!(
        io.kind(),
        K::ConnectionReset | K::ConnectionAborted | K::BrokenPipe | K::UnexpectedEof
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses_pass() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.1.5",
            "172.16.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:192.168.1.1",
        ] {
            assert!(!public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["93.184.216.34", "2606:4700::1111", "::ffff:8.8.8.8"] {
            assert!(public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn the_public_agent_refuses_this_pc() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", l.local_addr().unwrap());
        let e = public_agent(PAGE).get(&url).call().unwrap_err();
        assert!(e.to_string().contains("not a public"), "{e}");
        let url = url.replace("127.0.0.1", "localhost");
        assert!(public_agent(PAGE).get(&url).call().is_err());
    }

    #[test]
    fn a_reset_is_dropped_but_offline_is_not() {
        use std::io::{Error, ErrorKind as K};
        let io = |k| ureq::Error::Io(Error::from(k));
        for k in [
            K::ConnectionReset,
            K::ConnectionAborted,
            K::BrokenPipe,
            K::UnexpectedEof,
        ] {
            assert!(dropped(&io(k)), "{k:?}");
        }
        for e in [
            io(K::ConnectionRefused),
            io(K::HostUnreachable),
            ureq::Error::HostNotFound,
            ureq::Error::ConnectionFailed,
        ] {
            assert!(!dropped(&e), "{e}");
        }
    }

    /// (policy, connect, global, https_only, status_as_error, custom UA): the
    /// values each caller had before the builders were merged.
    #[test]
    fn policies_match_the_old_builders() {
        let d = |s| Some(Duration::from_secs(s));
        let table = [
            (AI, d(10), d(300), false, false, false),
            (MODELS, d(30), d(600), true, false, false),
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
