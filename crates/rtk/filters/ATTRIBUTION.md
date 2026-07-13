# RTK filter attribution

The JSON filter files in this directory are vendored verbatim from **OmniRoute**
(`open-sse/services/compression/engines/rtk/filters/`), MIT-licensed, copyright
(c) 2026 diegosouzapw — https://github.com/diegosouzapw/OmniRoute

OmniRoute's RTK engine is itself inspired by **RTK — Rust Token Killer**
(https://github.com/rtk-ai/rtk), Apache-2.0.

Each filter is data-only (regex + declarative line operations) and carries inline
`tests[]` samples. Those samples are executed byte-for-byte by the crate's
conformance suite (`tests/conformance.rs`) to prove the Rust `applyLineFilter`
port matches OmniRoute's behavior.

MIT license text: see the workspace root `LICENSE`.
