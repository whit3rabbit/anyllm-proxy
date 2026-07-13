use super::utils::*;
use super::ToolGuardrailNudge;
use crate::tools::execution::ToolCall;
use anyllm_translate::anthropic::Tool;

pub(super) fn available_lsp_tools(tool_specs: &[Tool]) -> Vec<String> {
    let supported = [
        "find_definition",
        "find_references",
        "get_hover",
        "document_symbols",
        "workspace_symbols",
    ];
    tool_specs
        .iter()
        .filter(|tool| supported.contains(&tool.name.as_str()))
        .map(|tool| tool.name.clone())
        .collect()
}

pub(super) fn lsp_first_nudge(call: &ToolCall, lsp_tools: &[String]) -> Option<ToolGuardrailNudge> {
    let tool_name = call.name.to_ascii_lowercase();
    let args = object_args(&call.input);
    let symbol = if is_shell_tool(&tool_name) {
        shell_grep_symbol(command_arg(args?)?)?
    } else if is_grep_tool(&tool_name) {
        string_arg(
            args?,
            &["symbol", "name", "pattern", "query", "regex", "needle"],
        )
        .and_then(symbol_from_search_value)?
    } else if is_glob_tool(&tool_name) {
        string_arg(args?, &["pattern", "query", "glob"]).and_then(symbol_from_search_value)?
    } else {
        return None;
    };

    let tools = lsp_tools.join(", ");
    let fingerprint = format!("lsp_first:{}:{symbol}", call.name);
    Some(ToolGuardrailNudge {
        call_id: call.id.clone(),
        kind: "lsp_first",
        content: format!(
            "Use available LSP tools for symbol lookup instead of grep/glob/shell search. Available LSP tools: {tools}. Retry with the best matching LSP tool for `{symbol}`."
        ),
        fingerprint,
    })
}
