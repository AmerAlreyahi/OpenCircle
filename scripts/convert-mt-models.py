#!/usr/bin/env python3
"""Converts Helsinki-NLP's own OPUS-MT models to the ONNX layout the app's
offline translator reads (`src/features/translate/mt`): the same files Xenova's transformers.js
exports have (int8 encoder + optimum's merged decoder, source.spm, vocab.json).

    uv venv -p 3.12 .venv
    uv pip install --index-url https://download.pytorch.org/whl/cpu torch==2.5.1
    uv pip install transformers==4.46.3 "optimum[onnxruntime]==1.23.3" \
        onnxruntime==1.20.1 onnx==1.17.0 sentencepiece==0.2.0 accelerate==1.1.1
    .venv/bin/python scripts/convert-mt-models.py OUT_DIR [pair ...]

(accelerate: without it optimum doesn't share the tied output layer and the
decoder is 92 MB instead of 59 MB.) Converting en-ar gives Xenova's layout and
sizes within a few hundred bytes (its int8 output matched the PyTorch
model on 4 of 5 test lines). A second run gives the same weights and graph, but torch
numbers some tensor names differently ("onnx::MatMul_1496" / "_1497"), so the
decoder's bytes (and sha256) differ: the app pins the published files' hashes.

Sources are pinned (REVISIONS); OUT_DIR/<pair>/ gets the four files and
OUT_DIR/manifest.json lists them (file, size, sha256, source, revision,
licence) and OUT_DIR/release/ holds the files under their published names,
`opus-mt-<pair>-<file>` (one flat release, models-v1); `src/features/translate/mt` pins the same
sizes and hashes.
"""

import hashlib
import json
import shutil
import sys
import tempfile
from pathlib import Path

from huggingface_hub import snapshot_download
from onnxruntime.quantization import QuantType, quantize_dynamic
from optimum.exporters.onnx import main_export

# Helsinki-NLP/opus-mt-<pair> at the commit converted (Tatoeba-Challenge /
# OPUS-MT weights, CC BY 4.0; the Hugging Face card says Apache-2.0). de-ar
# (d5d9d874) was converted and measured too: worse than through English.
REVISIONS = {
    "fr-ar": "5469dc759cd9fd856e45b0a916001efe44d1e700",
    "es-ar": "76efb7da47ad1ad8169635a847c1bd9d1c1815a7",
    "ar-fr": "e507d7bc850855f060e6bbe4751bb03141b99b7d",
    "ar-es": "da86e973e723a8196adc90ed17119da2e096c776",
    "ar-de": "8e6cbcce68feaf4eed2b926eccd31bfca97fab05",
    # Only to check this script against Xenova's export of the same model.
    "en-ar": "03087980e8ce753d64b3248ed0a912444545b840",
}
SOURCE_FILES = ["config.json", "generation_config.json", "pytorch_model.bin", "source.spm",
                "target.spm", "vocab.json", "tokenizer_config.json"]


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def quantize(src: Path, dst: Path) -> None:
    # As transformers.js quantizes: dynamic, per channel, reduced range, int8
    # MatMul weights, If-subgraphs (the merged decoder) included.
    quantize_dynamic(
        model_input=src,
        model_output=dst,
        per_channel=True,
        reduce_range=True,
        weight_type=QuantType.QInt8,
        extra_options={"EnableSubgraph": True},
    )


def convert(pair: str, out: Path) -> list[dict]:
    repo = f"Helsinki-NLP/opus-mt-{pair}"
    rev = REVISIONS[pair]
    src = Path(snapshot_download(repo, revision=rev, allow_patterns=SOURCE_FILES))
    dst = out / pair
    dst.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        main_export(str(src), tmp, task="text2text-generation-with-past", opset=11,
                    do_validation=False)
        quantize(tmp / "encoder_model.onnx", dst / "encoder_model_quantized.onnx")
        quantize(tmp / "decoder_model_merged.onnx", dst / "decoder_model_merged_quantized.onnx")
    for name in ("source.spm", "vocab.json"):
        shutil.copyfile(src / name, dst / name)
    licence = "CC-BY-4.0 (OPUS-MT / Tatoeba-Challenge weights; Hugging Face card: Apache-2.0)"
    return [
        {
            "pair": pair,
            "file": name,
            "asset": f"opus-mt-{pair}-{name}",
            "size": (dst / name).stat().st_size,
            "sha256": sha256(dst / name),
            "source": f"https://huggingface.co/{repo}",
            "revision": rev,
            "licence": licence,
        }
        for name in ("encoder_model_quantized.onnx", "decoder_model_merged_quantized.onnx",
                     "source.spm", "vocab.json")
    ]


def main() -> None:
    out = Path(sys.argv[1])
    pairs = sys.argv[2:] or [p for p in REVISIONS if p != "en-ar"]
    manifest = out / "manifest.json"
    rows = json.loads(manifest.read_text()) if manifest.exists() else []
    for pair in pairs:
        rows = [r for r in rows if r["pair"] != pair] + convert(pair, out)
        manifest.write_text(json.dumps(rows, indent=1) + "\n")
        (out / "release").mkdir(exist_ok=True)
        for r in rows:
            asset = out / "release" / r["asset"]
            asset.unlink(missing_ok=True)
            asset.hardlink_to(out / r["pair"] / r["file"])
        print(pair, "done")


if __name__ == "__main__":
    main()
