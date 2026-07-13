#!/usr/bin/env python3
"""
One-off generator for crates/optimizer/fixtures/parity/*.

Runs the REAL Python LLMLingua-2 reference implementation
(microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank via the
`llmlingua` PyPI package) over representative inputs for each category listed
in fixtures/README.md, and records `{input, ratio, keep_mask, compressed}`
per ALGO's planned parity format.

Not part of the Rust workspace / CI. Re-run manually (with `llmlingua`
installed in a venv) to regenerate fixtures if the reference model/library
version changes.
"""
import json
import os
from pathlib import Path

os.environ.setdefault("HF_HOME", "/tmp/hf_cache")

from llmlingua import PromptCompressor  # noqa: E402

OUT_ROOT = Path("/Users/whit3rabbit/Documents/GitHub/anyllm/crates/optimizer/fixtures/parity")
RATE = 0.5

SAMPLES = {
    "meeting": {
        "001": (
            "The quarterly budget meeting covered three major topics: staffing levels, "
            "the new office lease renewal, and the marketing campaign timeline for next "
            "quarter. The team agreed to revisit staffing numbers after the Q3 report is "
            "finalized. Action items were assigned to each department lead, and a "
            "follow-up meeting was scheduled for the second week of next month to review "
            "progress on the lease negotiation."
        ),
        "002": (
            "In today's standup, Priya reported that the payments service migration is "
            "on track for Friday's deploy window. Two blockers remain: the staging "
            "database needs a schema backfill, and QA has not yet signed off on the "
            "retry logic for failed webhook deliveries. Sam volunteered to pair with "
            "Priya this afternoon to close out the backfill script."
        ),
    },
    "markdown": {
        "001": (
            "# Release Notes\n\n"
            "## v2.3.0\n\n"
            "- Added support for streaming responses in the chat endpoint.\n"
            "- Fixed a bug where **retry backoff** could overflow on very large attempt "
            "counts.\n"
            "- Deprecated the legacy `/v1/complete` route in favor of `/v1/chat`.\n\n"
            "### Upgrade notes\n\n"
            "Existing integrations using the legacy route will continue to work until "
            "the next major version, but a deprecation warning is now logged on every "
            "call. See the [migration guide](https://example.com/migrate) for details."
        ),
    },
    "code": {
        "001": (
            "def compute_running_average(values):\n"
            "    \"\"\"Return the running average of a list of numbers.\"\"\"\n"
            "    total = 0\n"
            "    averages = []\n"
            "    for i, v in enumerate(values):\n"
            "        total += v\n"
            "        averages.append(total / (i + 1))\n"
            "    return averages\n\n"
            "# Example usage: prints the running average after each new sample.\n"
            "if __name__ == \"__main__\":\n"
            "    print(compute_running_average([1, 2, 3, 4, 5]))\n"
        ),
    },
    "json_in_prose": {
        "001": (
            "The API returned the following error payload when the request exceeded the "
            "rate limit: {\"error\": {\"type\": \"rate_limit_exceeded\", \"message\": "
            "\"You have exceeded your quota for this billing period\", \"retry_after\": "
            "30}}. Clients should back off using the retry_after value in seconds before "
            "issuing another request, and should log the error type for observability."
        ),
    },
    "multilingual": {
        "001": (
            "The customer support ticket was originally written in French: "
            "\"Bonjour, je n'arrive pas a me connecter a mon compte depuis hier soir.\" "
            "The support agent replied in English confirming the outage: \"Thank you for "
            "reaching out, we are aware of an authentication service disruption and are "
            "working on a fix.\" A follow-up update was later posted in Spanish: "
            "\"El problema ha sido resuelto, por favor intente iniciar sesion de nuevo.\""
        ),
    },
    "edge_cases": {
        "001": (
            "Deployment status update \U0001F680: the canary rollout finished with zero "
            "errors ✅. Logs from the Tokyo region (東京) show latency at "
            "45ms, while the Seoul region (서울) reports 52ms. Engineering will "
            "monitor for another hour before promoting to 100% traffic \U0001F440."
        ),
    },
}


def main():
    pc = PromptCompressor(
        model_name="microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank",
        use_llmlingua2=True,
        device_map="cpu",
    )

    for category, items in SAMPLES.items():
        cat_dir = OUT_ROOT / category
        cat_dir.mkdir(parents=True, exist_ok=True)
        for fixture_id, text in items.items():
            res = pc.compress_prompt(
                [text],
                rate=RATE,
                return_word_label=True,
                use_context_level_filter=False,
                use_sentence_level_filter=False,
            )
            labeled = res["fn_labeled_original_prompt"]
            words = []
            keep_mask = []
            for tok in labeled.split("\t\t|\t\t"):
                tok = tok.strip()
                if not tok:
                    continue
                word, label = tok.rsplit(" ", 1)
                words.append(word)
                keep_mask.append(label.strip() == "1")

            out = {
                "input": text,
                "ratio": RATE,
                "words": words,
                "keep_mask": keep_mask,
                "compressed": res["compressed_prompt"],
                "reference": {
                    "model": "microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank",
                    "llmlingua_version": "0.2.2",
                },
            }
            out_path = cat_dir / f"{fixture_id}.json"
            out_path.write_text(json.dumps(out, indent=2, ensure_ascii=False) + "\n")
            print(f"wrote {out_path} ({len(words)} words, kept {sum(keep_mask)})")


if __name__ == "__main__":
    main()
