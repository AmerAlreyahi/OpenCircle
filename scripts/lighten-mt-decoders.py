#!/usr/bin/env python3
"""Rewrites every pinned OPUS-MT decoder (`mt::PAIRS`, Xenova's and ours) with
its output layer built once, outside the merged decoder's two branches.

optimum's merged decoder is an `If` (first step / steps with past keys);
EACH branch holds `Transpose -> DequantizeLinear(model.shared.weight, int8)`
for the logits. The app runs ORT with `session.disable_quant_qdq = 1`, which
folds that to float at load: once PER BRANCH, two 62k x 512 float copies
(~2 x 128 MB). Here the two nodes (and their initializers) move to the main
graph, before the `If`; the branches read the outer value. Nothing else
changes: same inputs and outputs, same int8 weights, same translations.

    uv venv -p 3.12 .venv
    uv pip install onnx==1.23.2 onnxruntime==1.28.0 numpy sentencepiece
    .venv/bin/python scripts/lighten-mt-decoders.py CACHE OUT [pair ...]
    .venv/bin/python scripts/lighten-mt-decoders.py CACHE OUT --check [pair ...]
    .venv/bin/python scripts/lighten-mt-decoders.py --hoist IN.onnx OUT.onnx

--hoist lightens one freshly converted decoder (a pair added after models-v2,
whose v1 decoder was never pinned: models-v3's Portuguese).

CACHE gets each pair's pinned files (downloaded once, sha256 checked).
OUT gets `opus-mt-<pair>-decoder_model_merged_quantized.onnx` (one flat
release, models-v2) and SHA256SUMS.txt; the same bytes on every run.
--check greedy-decodes a few fixed sentences with the old and the new decoder
(the app's ORT settings) and prints whether the token ids are identical;
--mem PAIR... prints the resident memory after load, old vs new (one process
each). Then pin OUT's sizes and hashes in `mt::PAIRS`.
"""

import hashlib
import json
import re
import subprocess
import sys
import urllib.request
from pathlib import Path

import onnx

ROOT = Path(__file__).resolve().parent.parent
# The last commit whose `mt::PAIRS` pins the original (v1) decoders.
V1 = "d5c4e548255dfbf44c1368035fa0640665553d08"
DEC = "decoder_model_merged_quantized.onnx"
# A few lines per source language (the pivot pairs' too: x-en, en-y).
SENTENCES = {
    "en": ["The lighthouse stands on the coast.", "Press E to interact",
           "Are you sure you want to quit? Unsaved progress will be lost."],
    "ar": ["المنارة تقف على الساحل.", "اضغط على زر البدء", "هل أنت متأكد أنك تريد الخروج؟"],
    "fr": ["Le phare se dresse sur la côte.", "Appuyez sur E pour interagir",
           "Voulez-vous vraiment quitter la partie ?"],
    "es": ["El faro está en la costa.", "Pulsa E para interactuar", "¿Seguro que quieres salir?"],
    "de": ["Der Leuchtturm steht an der Küste.", "Drücke E, um zu interagieren",
           "Willst du das Spiel wirklich beenden?"],
    "it": ["Il faro si trova sulla costa.", "Premi E per interagire", "Vuoi davvero uscire?"],
    "nl": ["De vuurtoren staat aan de kust.", "Druk op E om te praten", "Wil je echt stoppen?"],
    "ru": ["Маяк стоит на берегу моря.", "Нажмите E, чтобы взаимодействовать",
           "Вы уверены, что хотите выйти?"],
    "uk": ["Маяк стоїть на березі моря.", "Натисніть E, щоб взаємодіяти", "Ви впевнені, що хочете вийти?"],
    "pl": ["Latarnia morska stoi na wybrzeżu.", "Naciśnij E, aby wejść w interakcję",
           "Czy na pewno chcesz wyjść?"],
    "zh": ["灯塔矗立在海岸上。", "按E键互动", "你确定要退出吗？"],
    "ja": ["灯台は海岸に立っています。", "Eキーで調べる", "本当に終了しますか？"],
    "ko": ["등대는 해안에 있습니다.", "E 키를 눌러 상호작용", "정말 종료하시겠습니까?"],
    "hi": ["प्रकाशस्तंभ तट पर खड़ा है।", "बातचीत करने के लिए E दबाएँ", "क्या आप सचमुच बाहर निकलना चाहते हैं?"],
    "id": ["Mercusuar berdiri di pantai.", "Tekan E untuk berinteraksi", "Yakin ingin keluar?"],
    "vi": ["Ngọn hải đăng đứng trên bờ biển.", "Nhấn E để tương tác", "Bạn có chắc muốn thoát không?"],
    "th": ["ประภาคารตั้งอยู่บนชายฝั่ง", "กด E เพื่อโต้ตอบ", "คุณแน่ใจหรือไม่ว่าต้องการออก"],
}


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def pins() -> dict:
    """Every pair of `mt::PAIRS` as models-v1 pinned it (`V1`): its four files
    (size, sha256, url), lang token, pad."""
    src = subprocess.check_output(["git", "-C", str(ROOT), "show", f"{V1}:src/mt/mod.rs"], text=True)
    table = src[src.index("pub static PAIRS"):]
    out = {}
    for m in re.finditer(r"pair!\(\s*(own,\s*)?(.*?)\n    \)", table, re.S):
        own = bool(m.group(1))
        lits = re.findall(r'"([^"]*)"|\b(\d[\d_]*)\b|\b(None)\b', m.group(2))
        vals = [a or b or c for a, b, c in lits]
        s, t, pair, rev = vals[:4]
        sizes = vals[4:12]
        names = ["encoder_model_quantized.onnx", DEC, "source.spm", "vocab.json"]
        paths = ["onnx/" + names[0], "onnx/" + DEC, "source.spm", "vocab.json"]
        files = {}
        for i, name in enumerate(names):
            if own:
                url = ("https://github.com/AmerAlreyahi/OpenCircle-models/releases/download/"
                       f"models-v1/opus-mt-{pair}-{name}")
            else:
                url = f"https://huggingface.co/Xenova/opus-mt-{pair}/resolve/{rev}/{paths[i]}"
            files[name] = (int(sizes[2 * i].replace("_", "")), sizes[2 * i + 1], url)
        token = None if vals[12] == "None" else vals[12]
        out[pair] = {"files": files, "token": token, "pad": int(vals[-1])}
    assert len(out) == 49, len(out)
    return out


def fetch(cache: Path, pair: str, p: dict) -> Path:
    d = cache / pair
    d.mkdir(parents=True, exist_ok=True)
    for name, (size, sha, url) in p["files"].items():
        f = d / name
        if f.exists() and f.stat().st_size == size and sha256(f) == sha:
            continue
        tmp = f.with_suffix(".part")
        urllib.request.urlretrieve(url, tmp)
        assert tmp.stat().st_size == size and sha256(tmp) == sha, f"{pair}/{name}: pin mismatch"
        tmp.rename(f)
    return d


def hoist(src: Path, dst: Path) -> None:
    m = onnx.load(str(src))
    g = m.graph
    iff = [n for n in g.node if n.op_type == "If"]
    assert len(iff) == 1
    iff = iff[0]
    branches = [a.g for a in iff.attribute if a.type == onnx.AttributeProto.GRAPH]
    assert len(branches) == 2
    outer = {i.name for i in g.initializer}
    moved = None
    for b in branches:
        # the logits' weight: the one DequantizeLinear fed by a Transpose
        dq = [n for n in b.node if n.op_type == "DequantizeLinear"
              and any(t.op_type == "Transpose" and t.output[0] == n.input[0] for t in b.node)]
        assert len(dq) == 1, [n.output[0] for n in dq]
        dq = dq[0]
        tr = next(t for t in b.node if t.output[0] == dq.input[0])
        local = {i.name: i for i in b.initializer}
        consts = list(tr.input) + list(dq.input[1:])
        # every input is a constant: an initializer of the branch or the main graph
        assert all(n in local or n in outer for n in consts), consts
        if moved is None:
            moved = (onnx.helper.printable_node(tr), onnx.helper.printable_node(dq))
            for n in consts:
                if n in local:
                    assert n not in outer
                    g.initializer.append(local[n])
                    outer.add(n)
            at = list(g.node).index(iff)
            g.node.insert(at, dq)
            g.node.insert(at, tr)
        else:
            # the other branch built exactly the same value
            assert moved == (onnx.helper.printable_node(tr), onnx.helper.printable_node(dq))
            for n in consts:
                if n in local:
                    assert local[n].SerializeToString() == next(
                        i for i in g.initializer if i.name == n).SerializeToString()
        keep = [n for n in b.node if n is not dq and n is not tr]
        del b.node[:]
        b.node.extend(keep)
        for n in consts:
            if n in local:
                b.initializer.remove(local[n])
    onnx.checker.check_model(m)
    tmp = dst.with_suffix(".part")
    onnx.save(m, str(tmp))
    tmp.rename(dst)


def session(path: Path):
    import onnxruntime as ort
    so = ort.SessionOptions()
    so.enable_mem_pattern = False
    so.enable_cpu_mem_arena = False
    so.intra_op_num_threads = 4
    so.add_session_config_entry("session.disable_quant_qdq", "1")
    return ort.InferenceSession(str(path), so, providers=["CPUExecutionProvider"])


class Tok:
    def __init__(self, d: Path, p: dict):
        import sentencepiece as spm
        self.sp = spm.SentencePieceProcessor(model_file=str(d / "source.spm"))
        self.vocab = json.loads((d / "vocab.json").read_text())
        self.inv = {v: k for k, v in self.vocab.items()}
        self.unk = self.vocab["<unk>"]
        self.token = p["token"]

    def encode(self, text: str) -> list[int]:
        ids = [self.vocab.get(x, self.unk) for x in self.sp.encode(text, out_type=str)]
        return ([self.vocab[self.token]] if self.token else []) + ids + [0]

    def decode(self, ids: list[int]) -> str:
        return "".join(self.inv[i] for i in ids if i != 0).replace("▁", " ").strip()


def greedy(enc, dec, rows: list[list[int]], pad: int, steps: int = 64) -> list[list[int]]:
    """The app's decoding (`mt::Translator::run`): one batch, padded, greedy."""
    import numpy as np
    width = max(map(len, rows))
    ids = np.full((len(rows), width), pad, np.int64)
    mask = np.zeros_like(ids)
    for i, r in enumerate(rows):
        ids[i, :len(r)] = r
        mask[i, :len(r)] = 1
    hs = enc.run(None, {"input_ids": ids, "attention_mask": mask})[0]
    names = [i.name for i in dec.get_inputs()]
    past = {n: np.zeros((len(rows), 8, 0, 64), np.float32)
            for n in names if n.startswith("past_key_values")}
    outs = [o.name for o in dec.get_outputs()]
    cur = np.full((len(rows), 1), pad, np.int64)
    got = [[] for _ in rows]
    done = [False] * len(rows)
    for step in range(steps):
        feed = {"encoder_attention_mask": mask, "input_ids": cur,
                "encoder_hidden_states": hs, "use_cache_branch": np.array([step > 0])}
        feed.update(past)
        res = dec.run(None, feed)
        nxt = res[0][:, -1, :].argmax(-1)
        for i, t in enumerate(nxt):
            if not done[i]:
                got[i].append(int(t))
                done[i] = t == 0
        if all(done):
            break
        for o, v in zip(outs[1:], res[1:]):
            k = o.replace("present", "past_key_values")
            if step == 0 or ".decoder." in k:
                past[k] = v
        cur = nxt[:, None].astype(np.int64)
    return got


def check(cache: Path, out: Path, pair: str, p: dict) -> dict:
    d = cache / pair
    tok = Tok(d, p)
    enc = session(d / "encoder_model_quantized.onnx")
    rows = [tok.encode(s) for s in SENTENCES[pair.split("-")[0]]]
    old = greedy(enc, session(d / DEC), rows, p["pad"])
    new = greedy(enc, session(out / f"opus-mt-{pair}-{DEC}"), rows, p["pad"])
    return {"pair": pair, "identical": old == new, "lines": len(rows),
            "tokens": sum(map(len, old)), "out": [tok.decode(o) for o in new]}


def mem(cache: Path, pair: str, dec: Path) -> None:
    """Child: RSS (MB) the pair's two sessions add, after load and after a
    translation (trimmed), and the peak."""
    import ctypes
    import gc

    def status(field):
        for line in open("/proc/self/status"):
            if line.startswith(field):
                return int(line.split()[1]) / 1024

    def trimmed():
        gc.collect()
        ctypes.CDLL("libc.so.6").malloc_trim(0)
        return status("VmRSS") - base
    import onnxruntime  # noqa: F401  (loaded before the baseline)
    p = pins()[pair]
    tok = Tok(cache / pair, p)
    base = status("VmRSS")
    enc, d = session(cache / pair / "encoder_model_quantized.onnx"), session(dec)
    load = trimmed()
    greedy(enc, d, [tok.encode(s) for s in SENTENCES[pair.split("-")[0]]], p["pad"])
    print(json.dumps({"load": round(load), "run": round(trimmed()),
                      "peak": round(status("VmHWM") - base)}))


def main() -> None:
    if sys.argv[1] == "--mem-child":
        return mem(Path(sys.argv[2]), sys.argv[3], Path(sys.argv[4]))
    if sys.argv[1] == "--hoist":
        return hoist(Path(sys.argv[2]), Path(sys.argv[3]))
    cache, out = Path(sys.argv[1]), Path(sys.argv[2])
    args = sys.argv[3:]
    mode = args.pop(0) if args and args[0].startswith("--") else None
    table = pins()
    pairs = args or list(table)
    if mode == "--pins":
        return print(json.dumps({pr: table[pr] for pr in pairs}, indent=1))
    out.mkdir(parents=True, exist_ok=True)
    if mode == "--check":
        rows = [check(cache, out, pr, table[pr]) for pr in pairs]
        for r in rows:
            print(json.dumps(r, ensure_ascii=False))
        assert all(r["identical"] for r in rows)
        return
    if mode == "--mem":
        for pr in pairs:
            old, new = (json.loads(subprocess.check_output(
                [sys.executable, __file__, "--mem-child", str(cache), pr, str(dec)], text=True))
                for dec in (cache / pr / DEC, out / f"opus-mt-{pr}-{DEC}"))
            print(json.dumps({"pair": pr, "old_mb": old, "new_mb": new}))
        return
    for pr in pairs:
        d = fetch(cache, pr, table[pr])
        dst = out / f"opus-mt-{pr}-{DEC}"
        hoist(d / DEC, dst)
        assert dst.stat().st_size != table[pr]["files"][DEC][0], f"{pr}: same size as v1"
        print(pr, dst.stat().st_size, flush=True)
    sums = sorted(f"{sha256(f)}  {f.name}" for f in out.glob(f"opus-mt-*-{DEC}"))
    (out / "SHA256SUMS.txt").write_text("\n".join(sums) + "\n")


if __name__ == "__main__":
    main()
