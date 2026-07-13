#!/usr/bin/env python3
"""
Export + int8-quantize the LLMLingua-2 token-importance model to ONNX.

Model: microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank
       (~110M params, a mBERT token-classification head: per-subtoken
       binary "preserve" vs "discard" logits). This is the "-small" tier
       referenced in ALGO.md §6; XLM-R-large is an optional accuracy tier
       behind the same trait, not exported here.

This script is offline tooling only, run manually (or in a release job),
NOT part of `cargo build`. Its output (the .onnx file) is never committed
to the repo — see CLAUDE.md "Ship as a hash-pinned downloaded artifact
(MODEL_* env / config), NOT a bundled blob.". Instead:

  1. Run this script to produce a quantized .onnx file and its sha256.
  2. Upload that file to wherever the deploy target's MODEL_URL points
     (e.g. object storage, a GitHub release asset).
  3. Set MODEL_SHA256 (and MODEL_URL) and run the `optimize-model` CLI
     (crates/optimizer/optimize-cli/src/model_fetch.rs) to download +
     sha256-verify the artifact into MODEL_CACHE_DIR/<sha256>/. The scorer
     never auto-downloads; `LlmLingua2Scorer::from_files` loads that pair.

Usage:
  python crates/optimizer/scripts/export_llmlingua2.py
  python crates/optimizer/scripts/export_llmlingua2.py --output /tmp/llmlingua2.onnx

Requires (not part of the Rust workspace's deps; install in a venv):
  pip install torch transformers onnx onnxruntime

Output:
  Writes an int8-quantized ONNX file to --output (default:
  crates/optimizer/artifacts/llmlingua2-bert-base-multilingual-int8.onnx,
  gitignored) and prints its sha256 to stdout as:
    MODEL_SHA256=<hex>
  That value is what gets pinned into MODEL_SHA256 / PolicyVersion.
"""

import argparse
import hashlib
import sys
from pathlib import Path

MODEL_ID = "microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank"
DEFAULT_OUTPUT = (
    Path(__file__).resolve().parent.parent
    / "artifacts"
    / "llmlingua2-bert-base-multilingual-int8.onnx"
)
# CLS/SEP included; matches optimize-scorer's `max_seq` (ALGO §6).
MAX_SEQ_LEN = 512
OPSET = 17


def export_fp32(model_id: str, fp32_path: Path) -> None:
    """Trace the HF token-classification model and export it to ONNX (fp32)."""
    import torch
    from transformers import AutoModelForTokenClassification, AutoTokenizer

    tokenizer = AutoTokenizer.from_pretrained(model_id)
    model = AutoModelForTokenClassification.from_pretrained(model_id)
    model.eval()

    dummy = tokenizer(
        "export dummy input for onnx tracing",
        return_tensors="pt",
        padding="max_length",
        truncation=True,
        max_length=MAX_SEQ_LEN,
    )

    fp32_path.parent.mkdir(parents=True, exist_ok=True)
    torch.onnx.export(
        model,
        (dummy["input_ids"], dummy["attention_mask"], dummy["token_type_ids"]),
        str(fp32_path),
        input_names=["input_ids", "attention_mask", "token_type_ids"],
        output_names=["logits"],
        dynamic_axes={
            "input_ids": {0: "batch", 1: "sequence"},
            "attention_mask": {0: "batch", 1: "sequence"},
            "token_type_ids": {0: "batch", 1: "sequence"},
            "logits": {0: "batch", 1: "sequence"},
        },
        opset_version=OPSET,
    )
    # Tokenizer files (vocab.txt / tokenizer.json / tokenizer_config.json) are
    # loaded at runtime by the Rust `tokenizers` crate; ship them alongside the
    # .onnx artifact under the same MODEL_URL prefix.
    tokenizer.save_pretrained(str(fp32_path.parent / "tokenizer"))


def quantize_int8(fp32_path: Path, int8_path: Path) -> None:
    """Dynamic int8 quantization (weights only; matches ort's CPU EP)."""
    from onnxruntime.quantization import QuantType, quantize_dynamic

    int8_path.parent.mkdir(parents=True, exist_ok=True)
    quantize_dynamic(
        model_input=str(fp32_path),
        model_output=str(int8_path),
        weight_type=QuantType.QInt8,
    )


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help=f"path for the quantized .onnx artifact (default: {DEFAULT_OUTPUT})",
    )
    parser.add_argument(
        "--model-id",
        default=MODEL_ID,
        help=f"HF model id to export (default: {MODEL_ID})",
    )
    parser.add_argument(
        "--keep-fp32",
        action="store_true",
        help="keep the intermediate fp32 .onnx file (default: deleted after quantization)",
    )
    args = parser.parse_args()

    fp32_path = args.output.with_name(args.output.stem + ".fp32.onnx")

    print(f"[1/3] exporting {args.model_id} to ONNX (fp32) -> {fp32_path}")
    export_fp32(args.model_id, fp32_path)

    print(f"[2/3] int8-quantizing -> {args.output}")
    quantize_int8(fp32_path, args.output)

    if not args.keep_fp32:
        fp32_path.unlink(missing_ok=True)

    digest = sha256_of(args.output)
    print(f"[3/3] done: {args.output} ({args.output.stat().st_size} bytes)")
    # Machine-parseable line: pin this into MODEL_SHA256 (see optimize-scorer docs).
    print(f"MODEL_SHA256={digest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
