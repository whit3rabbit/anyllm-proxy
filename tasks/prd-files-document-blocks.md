# PRD: Phase 8 - Files and Document Blocks

## Introduction

Add support for translating Anthropic document and image content blocks to OpenAI's equivalent input formats. Anthropic clients can send PDFs as base64 document blocks and images as base64 or URL sources; these must be translated to OpenAI's `input_file.file_data` (Responses API) or structured content parts (Chat Completions).

## Goals

- Translate Anthropic `document` content blocks (base64 PDF) to OpenAI format
- Translate Anthropic `image` content blocks (base64 and URL) to OpenAI format
- Enforce the 32MB size limit on file content
- Support mixed content messages (text + images + documents)

## User Stories

### US-001: Image block translation
**Description:** As a client sending vision requests, I need image content blocks translated so OpenAI models can process them.

**Acceptance Criteria:**
- [ ] Anthropic `{type: "image", source: {type: "base64", media_type: "image/png", data: "..."}}` -> OpenAI Chat Completions `{type: "image_url", image_url: {url: "data:image/png;base64,..."}}`
- [ ] Anthropic `{type: "image", source: {type: "url", url: "https://..."}}` -> OpenAI `{type: "image_url", image_url: {url: "https://..."}}`
- [ ] Supported media types: `image/png`, `image/jpeg`, `image/gif`, `image/webp`
- [ ] Test: base64 image, URL image, different media types

### US-002: Document block translation
**Description:** As a client sending PDF documents, I need document blocks translated to OpenAI's file input format.

**Acceptance Criteria:**
- [ ] Anthropic `{type: "document", source: {type: "base64", media_type: "application/pdf", data: "..."}}` -> OpenAI Responses `input_file` with `file_data` containing the base64 data
- [ ] If using Chat Completions backend (no native file support), return an informative error or attempt best-effort text extraction note
- [ ] Test: PDF base64 document block translation

### US-003: Size limit enforcement
**Description:** As an operator, I need oversized file content rejected before it reaches OpenAI.

**Acceptance Criteria:**
- [ ] Base64 content decoded size checked against 32MB limit
- [ ] Oversized content -> 413 `request_too_large` error with descriptive message
- [ ] Check happens during translation, before sending to OpenAI
- [ ] Test: content just under limit passes, content over limit rejected

### US-004: Mixed content messages
**Description:** As a client, I need to send messages with text, images, and documents interleaved in a single message.

**Acceptance Criteria:**
- [ ] User message with [text, image, text] -> OpenAI message with corresponding content parts array
- [ ] User message with [text, document, text] -> appropriate handling per backend
- [ ] Content block ordering preserved
- [ ] Test: mixed content message with multiple block types

## Functional Requirements

- FR-1: Image translation supports both base64 and URL source types
- FR-2: Document translation targets OpenAI Responses API `input_file.file_data` format
- FR-3: Size validation runs before forwarding to prevent wasting upstream bandwidth
- FR-4: Unsupported media types produce clear error messages

## Non-Goals

- No file upload to OpenAI Files API (upload + reference workflow)
- No Anthropic beta Files API support
- No text extraction from PDFs
- No image resizing or format conversion

## Technical Considerations

- Base64 data URLs for Chat Completions use format `data:{media_type};base64,{data}`
- Size check: base64 string length * 3/4 gives approximate decoded size
- Document support may require the Responses API backend; if Chat Completions is the only backend, document blocks should produce a clear error
- Consider feature-gating document support behind a config flag

## Success Metrics

- Fixture tests for image and document block translation
- Integration test: request with image block produces correct OpenAI payload
- Size limit test: oversized content rejected with 413
- `cargo test` passes
