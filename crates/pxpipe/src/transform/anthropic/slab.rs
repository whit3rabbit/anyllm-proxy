use super::common::{
    image_block, prepend_images_to_first_user, read_system, render_tool_doc, AnthropicOpts,
    SLAB_HEADER,
};
use crate::render::{render_text, RenderOpts};
use crate::transform::factsheet;
use crate::transform::gate;
use crate::transform::info::TransformInfo;
use crate::transform::schema_strip;
use serde_json::Value;

/// Static system+tools slab pass. Returns the skip reason (or "applied"); only
/// mutates `root` / fills `info` when profitable.
pub(crate) fn apply_slab(
    root: &mut Value,
    opts: &AnthropicOpts,
    info: &mut TransformInfo,
    budget: &mut usize,
) -> &'static str {
    let (system_text, system_cc) = read_system(root.get("system"));

    // Capture a tool cache_control as a fallback anchor (tools sit before system
    // in the cache prefix, so system's wins when both exist).
    let tools_present = opts.compress_tools
        && root
            .get("tools")
            .and_then(|t| t.as_array())
            .is_some_and(|a| !a.is_empty());
    let mut tool_cc = None;
    let mut tool_ref = String::new();
    if tools_present {
        if let Some(arr) = root.get("tools").and_then(|t| t.as_array()) {
            for t in arr {
                tool_ref.push_str(&render_tool_doc(t));
                if let Some(c) = t.get("cache_control") {
                    tool_cc = Some(c.clone());
                }
            }
        }
    }

    // Assemble the slab: header + system + tool reference.
    let mut slab = String::with_capacity(system_text.len() + tool_ref.len() + 64);
    slab.push_str(SLAB_HEADER);
    slab.push_str(&system_text);
    if !tool_ref.is_empty() {
        slab.push_str("\n\n# Tool Reference\n");
        slab.push_str(&tool_ref);
    }

    let slab_chars = slab.chars().count();
    // Nothing meaningful to image (no system text and no tools).
    if system_text.trim().is_empty() && tool_ref.is_empty() {
        return "no_slab";
    }
    if slab_chars < opts.min_compress_chars {
        return "below_min_chars";
    }

    let images = render_text(
        &slab,
        RenderOpts {
            cols: opts.cols,
            max_height_px: opts.max_height_px,
        },
    );
    if !gate::is_profitable(&images, slab_chars, opts.chars_per_token) {
        return "not_profitable";
    }
    if images.len() > *budget {
        return "image_budget";
    }

    // ---- commit: mutate root ------------------------------------------------
    let anchor = system_cc.or(tool_cc); // relocate this onto the last image
    let fact = factsheet::fact_sheet_text(&slab);

    let mut image_blocks: Vec<Value> = images.iter().map(|im| image_block(&im.png)).collect();
    let image_bytes: usize = images.iter().map(|im| im.png.len()).sum();
    let image_pixels: usize = images
        .iter()
        .map(|im| im.width as usize * im.height as usize)
        .sum();
    let dropped: usize = images.iter().map(|im| im.dropped).sum();

    // Relocate the caller's cache breakpoint onto the LAST image so the whole
    // imaged prefix caches as one stable segment. pxpipe never *adds* a marker.
    let relocated = if let (Some(cc), Some(last)) = (anchor, image_blocks.last_mut()) {
        if let Some(obj) = last.as_object_mut() {
            obj.insert("cache_control".into(), cc);
        }
        true
    } else {
        false
    };

    let obj = root.as_object_mut().expect("checked is_object above");

    // 1. system -> short pointer (+ factsheet). Removes original cache_control
    //    (relocated onto the image) since we're replacing the whole field.
    let mut pointer = String::from(
        "[Your reference context for this session is provided as an image in the first user message. Read it there.]",
    );
    if !fact.is_empty() {
        pointer.push('\n');
        pointer.push_str(&fact);
    }
    obj.insert("system".into(), Value::String(pointer));

    // 2. tools -> stub description + annotation-stripped schema, drop the
    //    relocated cache_control. Keep name + structural schema so Anthropic's
    //    tool-use validator still accepts calls.
    if tools_present {
        if let Some(arr) = obj.get_mut("tools").and_then(|t| t.as_array_mut()) {
            for t in arr.iter_mut() {
                let Some(tm) = t.as_object_mut() else {
                    continue;
                };
                let name = tm
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                tm.insert(
                    "description".into(),
                    Value::String(format!("See \"## Tool: {name}\" in the reference image.")),
                );
                if let Some(schema) = tm.get("input_schema") {
                    let stripped = schema_strip::strip(schema);
                    if schema_strip::has_structure(&stripped) {
                        tm.insert("input_schema".into(), stripped);
                    }
                    // else keep original: a bare {type:object} stub causes 400s.
                }
                tm.remove("cache_control");
            }
        }
    }

    // 3. Prepend image blocks to the first user message (system rejects images).
    prepend_images_to_first_user(obj, &mut image_blocks);

    info.compressed_chars += slab.len();
    info.image_count += images.len();
    info.image_bytes += image_bytes;
    info.image_pixels += image_pixels;
    info.dropped_chars += dropped;
    info.relocated_cache_anchor = relocated;
    *budget -= images.len();
    "applied"
}
