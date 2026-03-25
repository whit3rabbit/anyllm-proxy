# Library Integration Guide

Research deliverable for Phase 21b. Evaluates how non-Rust consumers can use the `anyllm_translate` crate's translation logic without running the proxy as a separate process.

## Overview

The translator crate is pure Rust: no IO, no async, no network. All mapping is stateless `fn(A) -> B` (except streaming, which holds a small state machine). This makes it an ideal candidate for cross-language bindings.

**The core constraint:** public types use `serde_json::Value`, `#[serde(untagged)]` enums, `#[serde(flatten)]` with `serde_json::Map`, and deeply nested `Vec<ContentBlock>`. These cannot be represented as C structs. All FFI approaches must use **JSON strings as the boundary**: callers pass JSON in, get JSON back. This is consistent with how the proxy already works (HTTP JSON in/out) and avoids exposing Rust-specific type complexity.

| Approach | Target Languages | Build Tool | Package Format |
|---|---|---|---|
| C FFI (cbindgen) | Any with C FFI (Python, Node, Go, Ruby, Java) | cargo + cbindgen | `.so` / `.dylib` / `.dll` + `.h` |
| WASM (wasm-bindgen) | JS/TS, any WASM host | wasm-pack | `.wasm` + JS glue / npm package |
| PyO3 (maturin) | Python | maturin | wheel / PyPI package |

Dependencies that matter for cross-compilation:
- `serde`, `serde_json`, `thiserror`: all targets, no issues.
- `tracing`: all targets, no issues (events are no-ops without a subscriber).
- `uuid` v4: calls `getrandom` internally. Blocks `wasm32-unknown-unknown` without the `js` feature on `getrandom`.

## C FFI via cbindgen

### Concept

A thin `extern "C"` wrapper module that accepts and returns `*const c_char` (null-terminated JSON strings). The caller is responsible for freeing returned strings via a dedicated free function. Errors are stored in a thread-local and retrieved separately.

### Cargo.toml Changes

```toml
[lib]
crate-type = ["rlib", "cdylib"]  # rlib for Rust consumers, cdylib for shared library

[features]
ffi = []  # gate FFI wrapper code behind a feature
```

### Wrapper API (8 functions)

```rust
// crates/translator/src/ffi.rs (gated behind #[cfg(feature = "ffi")])

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::cell::RefCell;

thread_local! {
    static LAST_ERROR: RefCell<Option<String>> = RefCell::new(None);
}

fn set_error(msg: String) {
    LAST_ERROR.with(|e| *e.borrow_mut() = Some(msg));
}

/// Returns the last error message, or null if no error.
/// Caller must free the returned string with `translate_free_string`.
#[no_mangle]
pub extern "C" fn translate_last_error() -> *const c_char {
    LAST_ERROR.with(|e| {
        match e.borrow().as_ref() {
            Some(msg) => CString::new(msg.as_str()).unwrap().into_raw(),
            None => std::ptr::null(),
        }
    })
}

/// Translate an Anthropic request to OpenAI format.
/// config_json: TranslationConfig as JSON string.
/// request_json: Anthropic MessageCreateRequest as JSON string.
/// Returns OpenAI ChatCompletionRequest as JSON string, or null on error.
#[no_mangle]
pub extern "C" fn translate_request_ffi(
    config_json: *const c_char,
    request_json: *const c_char,
) -> *const c_char {
    let config_str = unsafe { CStr::from_ptr(config_json) }.to_str().unwrap();
    let request_str = unsafe { CStr::from_ptr(request_json) }.to_str().unwrap();

    let config: TranslationConfig = match serde_json::from_str(config_str) {
        Ok(c) => c,
        Err(e) => { set_error(e.to_string()); return std::ptr::null(); }
    };
    let request: MessageCreateRequest = match serde_json::from_str(request_str) {
        Ok(r) => r,
        Err(e) => { set_error(e.to_string()); return std::ptr::null(); }
    };

    match crate::translate_request(&request, &config) {
        Ok(openai_req) => {
            let json = serde_json::to_string(&openai_req).unwrap();
            CString::new(json).unwrap().into_raw()
        }
        Err(e) => { set_error(e.to_string()); std::ptr::null() }
    }
}

/// Translate an OpenAI response back to Anthropic format.
#[no_mangle]
pub extern "C" fn translate_response_ffi(
    response_json: *const c_char,
    original_model: *const c_char,
) -> *const c_char { /* similar pattern */ }

/// Create a new streaming translator. Returns an opaque handle.
#[no_mangle]
pub extern "C" fn stream_translator_new(
    model: *const c_char,
) -> *mut StreamingTranslator { /* Box::into_raw(Box::new(...)) */ }

/// Feed a chunk to the streaming translator.
/// Returns a JSON array of Anthropic SSE events, or null on error.
#[no_mangle]
pub extern "C" fn stream_translator_process_chunk(
    handle: *mut StreamingTranslator,
    chunk_json: *const c_char,
) -> *const c_char { /* deserialize chunk, call process_chunk, serialize events */ }

/// Finalize the streaming translator. Returns remaining events as JSON array.
#[no_mangle]
pub extern "C" fn stream_translator_finish(
    handle: *mut StreamingTranslator,
) -> *const c_char { /* call finish(), serialize events */ }

/// Free a streaming translator handle.
#[no_mangle]
pub extern "C" fn stream_translator_free(handle: *mut StreamingTranslator) {
    if !handle.is_null() { unsafe { drop(Box::from_raw(handle)); } }
}

/// Free a string returned by any translate function.
#[no_mangle]
pub extern "C" fn translate_free_string(ptr: *mut c_char) {
    if !ptr.is_null() { unsafe { drop(CString::from_raw(ptr)); } }
}
```

### cbindgen Configuration

```toml
# cbindgen.toml
language = "C"
header = "/* Auto-generated by cbindgen. Do not edit. */"
include_guard = "ANTHROPIC_OPENAI_TRANSLATE_H"
autogen_warning = "/* Warning: this file is auto-generated by cbindgen. */"

[export]
exclude = []  # only extern "C" functions are exported
```

Build: `cbindgen --config cbindgen.toml --crate anyllm_translate --output anyllm_translate.h`

### Language Binding Examples

**Python (ctypes):**

```python
import ctypes
import json

lib = ctypes.CDLL("./target/release/libanyllm_translate.so")
lib.translate_request_ffi.restype = ctypes.c_char_p
lib.translate_request_ffi.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
lib.translate_free_string.argtypes = [ctypes.c_char_p]

config = json.dumps({"model_map": [["haiku", "gpt-4o-mini"]], "lossy_behavior": "Warn"})
request = json.dumps({"model": "claude-haiku-3", "max_tokens": 1024, "messages": [...]})

result_ptr = lib.translate_request_ffi(config.encode(), request.encode())
if result_ptr:
    result = json.loads(result_ptr.decode())
    lib.translate_free_string(result_ptr)
```

**Node.js (koffi):**

```javascript
const koffi = require('koffi');
const lib = koffi.load('./target/release/libanyllm_translate.so');
const translate_request = lib.func('const char* translate_request_ffi(const char*, const char*)');
const free_string = lib.func('void translate_free_string(char*)');

const config = JSON.stringify({ model_map: [["haiku", "gpt-4o-mini"]] });
const request = JSON.stringify({ model: "claude-haiku-3", max_tokens: 1024, messages: [...] });
const result = translate_request(config, request);  // koffi handles string conversion
const parsed = JSON.parse(result);
```

**Go (cgo):**

```go
// #cgo LDFLAGS: -L./target/release -lanyllm_translate
// #include "anyllm_translate.h"
import "C"
import "unsafe"

func TranslateRequest(config, request string) (string, error) {
    cConfig := C.CString(config)
    defer C.free(unsafe.Pointer(cConfig))
    cRequest := C.CString(request)
    defer C.free(unsafe.Pointer(cRequest))

    result := C.translate_request_ffi(cConfig, cRequest)
    if result == nil {
        errMsg := C.translate_last_error()
        defer C.translate_free_string((*C.char)(unsafe.Pointer(errMsg)))
        return "", fmt.Errorf("%s", C.GoString(errMsg))
    }
    defer C.translate_free_string((*C.char)(unsafe.Pointer(result)))
    return C.GoString(result), nil
}
```

### Verdict

**Feasible.** The JSON boundary makes it straightforward. The main cost is per-language integration effort: each consumer needs its own FFI loading code, string marshaling, and memory management discipline. Platform-specific builds (linux/macos/windows) add CI complexity.

Best for: languages without WASM support, performance-critical native services, environments where a shared library is easier to deploy than a sidecar process.

## WASM via wasm-bindgen

### Concept

Compile to `wasm32-unknown-unknown` and expose functions via `wasm-bindgen`. Callers interact through JS-native strings and classes. A single `.wasm` artifact works in browsers, Node.js, Deno, and edge runtimes (Cloudflare Workers, Vercel Edge).

### Blocker: `uuid` v4 and `getrandom`

The `uuid` crate's `v4` feature calls `getrandom` for OS entropy. On `wasm32-unknown-unknown`, `getrandom` has no default entropy source. The fix:

```toml
# Cargo.toml (translator crate)
[target.'cfg(target_arch = "wasm32")'.dependencies]
getrandom = { version = "0.2", features = ["js"] }
```

This wires `crypto.getRandomValues()` as the entropy source. Requires a JS environment (browser or Node.js), which is always the case for `wasm-bindgen` targets.

Alternative (more complex): feature-gate `uuid` behind `cfg(not(target_arch = "wasm32"))` and accept IDs as input parameters in WASM mode. Not recommended; the `getrandom` js feature is simpler.

### Cargo.toml Changes

```toml
[lib]
crate-type = ["rlib", "cdylib"]  # cdylib needed for wasm-pack

[features]
wasm = ["wasm-bindgen"]

[dependencies]
wasm-bindgen = { version = "0.2", optional = true }

[target.'cfg(target_arch = "wasm32")'.dependencies]
getrandom = { version = "0.2", features = ["js"] }
```

### Wrapper API

```rust
// crates/translator/src/wasm.rs (gated behind #[cfg(feature = "wasm")])

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn translate_request(config_json: &str, request_json: &str) -> Result<String, JsValue> {
    let config: TranslationConfig = serde_json::from_str(config_json)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let request: MessageCreateRequest = serde_json::from_str(request_json)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    let openai_req = crate::translate_request(&request, &config)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(&openai_req)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn translate_response(response_json: &str, original_model: &str) -> Result<String, JsValue> {
    let response: ChatCompletionResponse = serde_json::from_str(response_json)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let anthropic_resp = crate::translate_response(&response, original_model);
    serde_json::to_string(&anthropic_resp)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub struct WasmStreamingTranslator {
    inner: StreamingTranslator,
}

#[wasm_bindgen]
impl WasmStreamingTranslator {
    #[wasm_bindgen(constructor)]
    pub fn new(model: &str) -> Self {
        Self { inner: crate::new_stream_translator(model.to_string()) }
    }

    /// Feed an OpenAI chunk (JSON string), returns Anthropic events (JSON array string).
    pub fn process_chunk(&mut self, chunk_json: &str) -> Result<String, JsValue> {
        let chunk: ChatCompletionChunk = serde_json::from_str(chunk_json)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let events = self.inner.process_chunk(&chunk);
        serde_json::to_string(&events)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn finish(&mut self) -> Result<String, JsValue> {
        let events = self.inner.finish();
        serde_json::to_string(&events)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
```

### Build and Packaging

```bash
# Install wasm-pack
cargo install wasm-pack

# Build for bundlers (webpack, vite)
wasm-pack build crates/translator --features wasm --target bundler

# Build for vanilla JS (no bundler)
wasm-pack build crates/translator --features wasm --target web

# Build for Node.js
wasm-pack build crates/translator --features wasm --target nodejs
```

Output: `crates/translator/pkg/` with `.wasm`, `.js` glue, `package.json`, TypeScript definitions.

### Usage Examples

**Browser (ES module):**

```javascript
import init, { translate_request, WasmStreamingTranslator } from '@anthropic-openai-translate/wasm';

await init();  // load WASM binary

const config = JSON.stringify({ model_map: [["haiku", "gpt-4o-mini"]] });
const request = JSON.stringify({ model: "claude-haiku-3", max_tokens: 1024, messages: [...] });
const openaiJson = translate_request(config, request);
const openaiReq = JSON.parse(openaiJson);

// Streaming
const translator = new WasmStreamingTranslator("gpt-4o");
for await (const chunk of openaiStream) {
    const eventsJson = translator.process_chunk(JSON.stringify(chunk));
    const events = JSON.parse(eventsJson);
    // emit events to client
}
const finalEvents = JSON.parse(translator.finish());
```

**Node.js:**

```javascript
const { translate_request } = require('@anthropic-openai-translate/wasm');
// No init() needed for Node.js target
const result = translate_request(configJson, requestJson);
```

**Cloudflare Worker:**

```javascript
import { translate_request } from '@anthropic-openai-translate/wasm';
export default {
    async fetch(request) {
        const body = await request.json();
        const openaiJson = translate_request(configJson, JSON.stringify(body));
        return fetch("https://api.openai.com/v1/chat/completions", {
            method: "POST",
            headers: { "Authorization": `Bearer ${env.OPENAI_API_KEY}` },
            body: openaiJson,
        });
    }
};
```

### Size Estimate

Similar pure-serde WASM crates compile to 500KB-2MB after `wasm-opt -Oz`. The `serde_json` dependency is the largest contributor. No way to avoid it since JSON parsing is core functionality.

### Verdict

**Feasible with one fix** (add `getrandom` js feature for WASM targets). Single artifact works everywhere JS runs. The main tradeoff is performance: serde in WASM is roughly 2-5x slower than native for JSON parsing. For a translation layer (not a hot loop), this is acceptable.

Best for: browser-based tools, edge runtimes (Cloudflare Workers, Deno Deploy), sandboxed environments, anywhere a native binary cannot be deployed.

## PyO3 Native Python Module

### Concept

Use `pyo3` and `maturin` to build a native Python extension module (`.so` on Linux/macOS, `.pyd` on Windows). Python sees it as a regular importable module. Published to PyPI as `pip install anthropic-openai-translate`.

### Cargo.toml Changes

```toml
[lib]
crate-type = ["rlib", "cdylib"]

[features]
python = ["pyo3"]

[dependencies]
pyo3 = { version = "0.22", features = ["extension-module"], optional = true }
```

### Wrapper API

```rust
// crates/translator/src/python.rs (gated behind #[cfg(feature = "python")])

use pyo3::prelude::*;
use pyo3::exceptions::PyValueError;

pyo3::create_exception!(anyllm_translate, TranslateError, pyo3::exceptions::PyException);

#[pyfunction]
fn translate_request(config_json: &str, request_json: &str) -> PyResult<String> {
    let config: TranslationConfig = serde_json::from_str(config_json)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let request: MessageCreateRequest = serde_json::from_str(request_json)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;

    let openai_req = crate::translate_request(&request, &config)
        .map_err(|e| TranslateError::new_err(e.to_string()))?;
    serde_json::to_string(&openai_req)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn translate_response(response_json: &str, original_model: &str) -> PyResult<String> {
    let response: ChatCompletionResponse = serde_json::from_str(response_json)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let anthropic_resp = crate::translate_response(&response, original_model);
    serde_json::to_string(&anthropic_resp)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyclass]
struct StreamingTranslator {
    inner: crate::mapping::streaming_map::StreamingTranslator,
}

#[pymethods]
impl StreamingTranslator {
    #[new]
    fn new(model: &str) -> Self {
        Self { inner: crate::new_stream_translator(model.to_string()) }
    }

    fn process_chunk(&mut self, chunk_json: &str) -> PyResult<String> {
        let chunk: ChatCompletionChunk = serde_json::from_str(chunk_json)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let events = self.inner.process_chunk(&chunk);
        serde_json::to_string(&events)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn finish(&mut self) -> PyResult<String> {
        let events = self.inner.finish();
        serde_json::to_string(&events)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

#[pymodule]
fn anyllm_translate(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(translate_request, m)?)?;
    m.add_function(wrap_pyfunction!(translate_response, m)?)?;
    m.add_class::<StreamingTranslator>()?;
    m.add("TranslateError", m.py().get_type::<TranslateError>())?;
    Ok(())
}
```

### Type Stubs

```python
# anyllm_translate.pyi
def translate_request(config_json: str, request_json: str) -> str: ...
def translate_response(response_json: str, original_model: str) -> str: ...

class StreamingTranslator:
    def __init__(self, model: str) -> None: ...
    def process_chunk(self, chunk_json: str) -> str: ...
    def finish(self) -> str: ...

class TranslateError(Exception): ...
```

### Build and Publishing

```bash
# Install maturin
pip install maturin

# Development build (installs into current venv)
maturin develop --features python

# Build wheel
maturin build --release --features python

# Publish to PyPI
maturin publish --features python
```

### Usage Example

```python
import json
import anyllm_translate as translator

config = json.dumps({
    "model_map": [["haiku", "gpt-4o-mini"], ["sonnet", "gpt-4o"]],
    "lossy_behavior": "Warn",
    "passthrough_unknown_models": True,
})

# Non-streaming
request = json.dumps({
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}],
})
openai_json = translator.translate_request(config, request)
openai_req = json.loads(openai_json)

# Send to OpenAI, get response...
anthropic_json = translator.translate_response(openai_response_json, "claude-sonnet-4-20250514")

# Streaming
stream = translator.StreamingTranslator("gpt-4o")
for chunk in openai_stream:
    events_json = stream.process_chunk(json.dumps(chunk))
    events = json.loads(events_json)
    for event in events:
        yield event
final_events = json.loads(stream.finish())
```

### Optional: Pure-Python Convenience Wrapper

A thin Python wrapper can accept/return dicts instead of JSON strings, hiding the serialization:

```python
# anyllm_translate/convenience.py (pure Python, wraps native module)
import json
import anyllm_translate._native as _native

def translate_request(config: dict, request: dict) -> dict:
    return json.loads(_native.translate_request(json.dumps(config), json.dumps(request)))
```

This adds one extra serialize/deserialize cycle but gives users a more Pythonic API. The performance cost is negligible compared to network latency.

### CI: Cross-Platform Wheels

`maturin` provides GitHub Actions templates for building wheels across platforms:

```yaml
# .github/workflows/python.yml (sketch)
jobs:
  build:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
        python: ["3.9", "3.10", "3.11", "3.12", "3.13"]
    steps:
      - uses: actions/checkout@v4
      - uses: PyO3/maturin-action@v1
        with:
          command: build
          args: --release --features python
```

### Verdict

**Feasible and recommended as the first FFI target.** Python is the dominant LLM ecosystem language. PyO3 + maturin is mature tooling with excellent cross-platform support. The JSON boundary keeps the wrapper thin (~50 lines of Rust). Publishing to PyPI gives broad reach.

Best for: Python codebases, data pipelines, LLM orchestration frameworks (LangChain, LlamaIndex), Jupyter notebooks.

## Integration Pattern Comparison

| | Rust Crate | C FFI | WASM | PyO3 | Proxy Sidecar |
|---|---|---|---|---|---|
| **When to use** | Rust codebases | Native polyglot services | Browser, edge, Node.js | Python codebases | Any language, quick start |
| **Latency** | Lowest (native call) | Low (call + JSON ser/de) | Medium (WASM + JSON) | Low (call + JSON ser/de) | High (HTTP round-trip) |
| **Integration complexity** | Lowest (cargo dep) | High (per-language FFI) | Medium (wasm-pack) | Low (pip install) | Lowest for consumer |
| **Deployment** | Compile-time | Ship .so/.dylib + .h | Ship .wasm + JS | pip wheel | Docker / binary |
| **Language support** | Rust only | Any with C FFI | JS/TS, WASM hosts | Python only | Any (HTTP) |
| **Streaming** | Native iterators | Opaque handle | JS class | Python class | SSE passthrough |
| **Ops overhead** | None | Build matrix | Single artifact | maturin CI matrix | Running process |
| **Binary size** | N/A (linked in) | ~2-5MB shared lib | 500KB-2MB .wasm | ~2-5MB wheel | ~10MB binary |

### Decision Tree

1. **Already using Rust?** Use the crate as a cargo dependency. Zero overhead, full type safety.
2. **Python project?** Use the PyO3 module. `pip install`, native speed, Pythonic exceptions.
3. **Browser or edge runtime?** Use WASM. Only option for client-side, single artifact.
4. **Other language (Go, Java, Ruby)?** Use C FFI if performance matters, proxy sidecar if simplicity matters.
5. **Don't want to write code?** Run the proxy as a sidecar (Docker or binary). Any language with an HTTP client works.

## Recommendations

**Implementation priority:** PyO3 > WASM > C FFI.

Rationale:
1. **PyO3 first.** Python dominates the LLM ecosystem. The tooling (maturin) is mature. Highest impact per effort.
2. **WASM second.** Browser and edge are growing deployment targets for LLM tools. Single artifact simplifies distribution.
3. **C FFI third (or skip).** High integration effort per language. Most non-Rust/non-Python/non-JS users are better served by the proxy sidecar. Implement only if specific demand exists.

For most users, the **proxy sidecar** (existing binary/Docker image) remains the simplest integration path. The FFI options serve users who cannot or prefer not to run a separate process.

## Known Blockers and Mitigations

| Blocker | Affects | Severity | Mitigation |
|---|---|---|---|
| `uuid` v4 needs `getrandom` with `js` feature | WASM | Build failure | Add `getrandom = { version = "0.2", features = ["js"] }` as a target-specific dependency for `wasm32` |
| `tracing` in non-Rust environments | All FFI | Non-issue | Events are no-ops without a subscriber. Consumers can optionally install `tracing-wasm` (WASM) or ignore entirely |
| Platform-specific builds | C FFI, PyO3 | CI complexity | Use `cross` (C FFI) or `maturin-action` (PyO3) in GitHub Actions for build matrix |
| `crate-type = ["cdylib"]` required | C FFI, WASM, PyO3 | Build config | Add `cdylib` alongside `rlib`. Feature-gate wrapper modules: `#[cfg(feature = "ffi")]`, `#[cfg(feature = "wasm")]`, `#[cfg(feature = "python")]` |
| Multiple cdylib targets conflict | All FFI | Build config | Only one FFI feature active per build. Separate CI jobs for each target |
| `serde_json::Value` in public types | All FFI | Design | Already solved by JSON string boundary. Types never cross FFI; only serialized JSON does |
