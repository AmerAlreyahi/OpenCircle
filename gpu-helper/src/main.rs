//! `opencircle-mt-gpu`: the GPU translation helper (llama.cpp, Vulkan).
//!
//! A separate exe so `opencircle.exe` never links llama.cpp. It reads one JSON request per
//! line on stdin and answers one JSON line on stdout (logs go to stderr, never stdout):
//!
//! - `{"op":"probe","model_bytes":1133080448}`: the Vulkan devices (kind, name, free VRAM)
//!   and whether the GPU rule holds: a discrete GPU with free VRAM >= model + 1 GiB.
//! - `{"op":"load","path":"model.gguf","n_ctx":1024,"gpu":true}`: load once per process
//!   (`gpu` false forces the CPU backend; absent = use the GPU when there is one).
//! - `{"op":"translate","text":"..","source":"en","target":"ar","glossary":[["term","term"]]}`
//! - `{"op":"quit"}`: no reply, the process exits (so does closing stdin).
//!
//! Replies: `{"ok":true,...}` or `{"ok":false,"error":"..."}`. Without a Vulkan driver the
//! probe says `"gpu":false` and the app keeps the CPU path.
use std::io::{BufRead, Write};
use std::num::NonZeroU32;
use std::time::Instant;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::{LlamaBackendDeviceType, list_llama_ggml_backend_devices};
use serde::Deserialize;
use serde_json::{Value, json};

/// Free VRAM the GPU rule keeps beside the model (owner, 2026-11-09: model + ~1 GB).
const HEADROOM: u64 = 1 << 30;
/// Generated tokens are bounded (W8 measured with 160): a game line is short.
const MAX_NEW: usize = 160;
const DEFAULT_CTX: u32 = 1024;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum Request {
    Probe {
        #[serde(default)]
        model_bytes: u64,
    },
    Load {
        path: String,
        n_ctx: Option<u32>,
        gpu: Option<bool>,
    },
    Translate {
        text: String,
        target: String,
        #[serde(default)]
        glossary: Vec<(String, String)>,
    },
    Quit,
}

/// Hy-MT2's documented English prompts (model card "Translation Task Instruction Examples"):
/// the default one, and the terminology one (reference lines first). `source` is not part of
/// the documented prompts; the model detects it, so a `source` field in a request is accepted and ignored.
fn prompt(text: &str, target: &str, glossary: &[(String, String)]) -> String {
    let lang = language_name(target);
    let task = format!(
        "Translate the following text into {lang}. Note that you should only output the \
         translated result without any additional explanation:\n\n{}",
        text.trim()
    );
    if glossary.is_empty() {
        return task;
    }
    let mut out = String::from("Reference the following translations:\n");
    for (from, to) in glossary {
        out.push_str(&format!("{from} translates to {to}\n"));
    }
    out.push('\n');
    out + &task
}

/// The full English name the model wants; an unknown code is passed through (a full name works).
fn language_name(code: &str) -> &str {
    match code {
        "en" => "English",
        "ar" => "Arabic",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "pt" => "Portuguese",
        "ru" => "Russian",
        "it" => "Italian",
        "tr" => "Turkish",
        "ja" => "Japanese",
        "ko" => "Korean",
        "zh" => "Chinese",
        "nl" => "Dutch",
        "pl" => "Polish",
        "uk" => "Ukrainian",
        "hi" => "Hindi",
        "id" => "Indonesian",
        "vi" => "Vietnamese",
        "th" => "Thai",
        other => other,
    }
}

struct Device {
    index: usize,
    kind: &'static str,
    name: String,
    free: u64,
    total: u64,
}

fn devices() -> Vec<Device> {
    list_llama_ggml_backend_devices()
        .into_iter()
        .filter_map(|d| {
            let kind = match d.device_type {
                LlamaBackendDeviceType::Gpu => "discrete",
                LlamaBackendDeviceType::IntegratedGpu => "integrated",
                // CPU and accelerators are not a GPU path.
                _ => return None,
            };
            Some(Device {
                index: d.index,
                kind,
                name: d.description,
                free: d.memory_free as u64,
                total: d.memory_total as u64,
            })
        })
        .collect()
}

/// The GPU rule: the discrete GPU with the most free VRAM, if that is >= model + headroom.
fn pick(devices: &[Device], model_bytes: u64) -> Option<&Device> {
    devices
        .iter()
        .filter(|d| d.kind == "discrete" && d.free >= model_bytes.saturating_add(HEADROOM))
        .max_by_key(|d| d.free)
}

fn probe(model_bytes: u64) -> Value {
    let devs = devices();
    let list: Vec<Value> = devs
        .iter()
        .map(|d| {
            json!({"index": d.index, "kind": d.kind, "name": d.name,
            "free_mb": d.free >> 20, "total_mb": d.total >> 20})
        })
        .collect();
    let fits = pick(&devs, model_bytes);
    json!({"ok": true, "gpu": !devs.is_empty(), "devices": list,
        "fits": fits.is_some(), "device": fits.map(|d| d.index)})
}

fn reply(out: &mut impl Write, v: Value) {
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn err(msg: impl std::fmt::Display) -> Value {
    json!({"ok": false, "error": msg.to_string()})
}

fn main() {
    let mut backend = match LlamaBackend::init() {
        Ok(b) => b,
        Err(e) => {
            reply(&mut std::io::stdout(), err(format!("backend init: {e}")));
            std::process::exit(1);
        }
    };
    backend.void_logs(); // llama.cpp's own chatter stays out of the pipe
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut lines = stdin.lock().lines();
    // Before a model is loaded: probe / load / quit. `load` moves into `serve`.
    while let Some(Ok(line)) = lines.next() {
        match serde_json::from_str::<Request>(&line) {
            Ok(Request::Quit) => return,
            Ok(Request::Probe { model_bytes }) => reply(&mut out, probe(model_bytes)),
            Ok(Request::Load { path, n_ctx, gpu }) => {
                let started = Instant::now();
                let use_gpu = gpu.unwrap_or(true) && !devices().is_empty();
                let mut params =
                    LlamaModelParams::default().with_n_gpu_layers(if use_gpu { 999 } else { 0 });
                if !use_gpu {
                    params = params.with_devices(&[]).unwrap_or_default();
                }
                let model = match LlamaModel::load_from_file(&backend, &path, &params) {
                    Ok(m) => m,
                    Err(e) => {
                        reply(&mut out, err(format!("load: {e}")));
                        continue;
                    }
                };
                let n_ctx = n_ctx.unwrap_or(DEFAULT_CTX).max(256);
                let cparams = LlamaContextParams::default()
                    .with_n_ctx(NonZeroU32::new(n_ctx))
                    .with_n_batch(n_ctx);
                let ctx = match model.new_context(&backend, cparams) {
                    Ok(c) => c,
                    Err(e) => {
                        reply(&mut out, err(format!("context: {e}")));
                        continue;
                    }
                };
                reply(
                    &mut out,
                    json!({"ok": true, "backend": if use_gpu { "gpu" } else { "cpu" },
                        "load_ms": started.elapsed().as_millis() as u64, "n_ctx": n_ctx}),
                );
                serve(&mut lines, &mut out, &model, ctx, n_ctx as usize);
                return;
            }
            Ok(Request::Translate { .. }) => reply(&mut out, err("no model loaded")),
            Err(e) => reply(&mut out, err(format!("bad request: {e}"))),
        }
    }
}

/// With a model loaded: translate / probe / quit (a second `load` is refused: one model per process).
fn serve(
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
    out: &mut impl Write,
    model: &LlamaModel,
    mut ctx: llama_cpp_2::context::LlamaContext<'_>,
    n_ctx: usize,
) {
    let mut batch = LlamaBatch::new(n_ctx, 1);
    while let Some(Ok(line)) = lines.next() {
        let v = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Quit) => return,
            Ok(Request::Probe { model_bytes }) => probe(model_bytes),
            Ok(Request::Load { .. }) => err("a model is already loaded; restart the helper"),
            Ok(Request::Translate {
                text,
                target,
                glossary,
            }) => translate(
                model,
                &mut ctx,
                &mut batch,
                &prompt(&text, &target, &glossary),
                n_ctx,
            )
            .unwrap_or_else(err),
            Err(e) => err(format!("bad request: {e}")),
        };
        reply(out, v);
    }
}

fn translate(
    model: &LlamaModel,
    ctx: &mut llama_cpp_2::context::LlamaContext<'_>,
    batch: &mut LlamaBatch<'_>,
    user: &str,
    n_ctx: usize,
) -> Result<Value, String> {
    let template = model
        .chat_template(None)
        .map_err(|e| format!("template: {e}"))?;
    let msg = LlamaChatMessage::new("user".into(), user.into()).map_err(|e| e.to_string())?;
    let text = model
        .apply_chat_template(&template, &[msg], true)
        .map_err(|e| format!("template: {e}"))?;
    let vocab = model.vocab();
    // The template text already begins with the begin-of-sentence token: no second BOS.
    let tokens = vocab.tokenize(text.as_bytes(), false, true);
    if tokens.is_empty() || tokens.len() + 8 > n_ctx {
        return Err(format!(
            "prompt of {} tokens does not fit the context of {n_ctx}",
            tokens.len()
        ));
    }
    ctx.clear_kv_cache();
    batch.clear();
    let last = tokens.len() - 1;
    for (i, t) in tokens.iter().enumerate() {
        batch
            .add(*t, i as i32, &[0], i == last)
            .map_err(|e| e.to_string())?;
    }
    let t0 = Instant::now();
    ctx.decode(batch).map_err(|e| format!("decode: {e}"))?;
    let prompt_ms = t0.elapsed().as_secs_f64() * 1e3;

    let mut sampler = LlamaSampler::greedy();
    let t1 = Instant::now();
    let mut bytes = Vec::new();
    let mut n_gen = 0usize;
    let mut pos = tokens.len();
    let mut idx = batch.n_tokens() - 1;
    while n_gen < MAX_NEW && pos < n_ctx {
        let tok = sampler.sample(ctx, idx);
        sampler.accept(tok);
        if vocab.is_eog(tok) {
            break;
        }
        bytes.extend(vocab.token_to_piece(tok, false, None));
        n_gen += 1;
        batch.clear();
        batch
            .add(tok, pos as i32, &[0], true)
            .map_err(|e| e.to_string())?;
        pos += 1;
        idx = 0;
        ctx.decode(batch).map_err(|e| format!("decode: {e}"))?;
    }
    let gen_ms = t1.elapsed().as_secs_f64() * 1e3;
    let text = String::from_utf8_lossy(&bytes).trim().replace('\n', " ");
    Ok(
        json!({"ok": true, "text": text, "prompt_ms": prompt_ms, "gen_ms": gen_ms,
        "prompt_tokens": tokens.len(), "gen_tokens": n_gen}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(kind: &'static str, free_mb: u64) -> Device {
        Device {
            index: 0,
            kind,
            name: String::new(),
            free: free_mb << 20,
            total: 0,
        }
    }

    #[test]
    fn gpu_rule_needs_discrete_and_model_plus_a_gigabyte() {
        let model = 1_133_080_448; // Q4_K_M, 1080 MiB
        assert!(pick(&[dev("discrete", 4000)], model).is_some());
        assert!(pick(&[dev("discrete", 2000)], model).is_none()); // 2000 < 1080 + 1024
        assert!(pick(&[dev("integrated", 16000)], model).is_none());
        assert!(pick(&[], model).is_none());
    }

    #[test]
    fn prompt_is_the_documented_one_with_optional_terminology() {
        let p = prompt(" Hello ", "ar", &[]);
        assert!(p.starts_with("Translate the following text into Arabic."));
        assert!(p.ends_with("explanation:\n\nHello"));
        let g = prompt("Hi", "fr", &[("Ronova".into(), "رونوفا".into())]);
        assert!(
            g.starts_with("Reference the following translations:\nRonova translates to رونوفا\n\n")
        );
        assert!(g.contains("into French."));
    }

    #[test]
    fn requests_parse() {
        let r: Request =
            serde_json::from_str(r#"{"op":"translate","text":"a","target":"ar"}"#).unwrap();
        assert!(matches!(r, Request::Translate { .. }));
        assert!(serde_json::from_str::<Request>(r#"{"op":"nope"}"#).is_err());
    }
}
