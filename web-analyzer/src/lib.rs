use analysis::Analysis;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
}

/// Parses a `.dem` file already loaded into memory (e.g. via a browser
/// `File`'s `arrayBuffer()`) and returns the same `Analysis` shape the
/// desktop app's `analyze_demo_full` Tauri command produces.
///
/// `progress_cb`, if given, is called as `(processed: number, total: number)`
/// while parsing — the browser equivalent of the desktop app's
/// `analyzer_progress` event.
#[wasm_bindgen(js_name = analyzeDemo)]
pub fn analyze_demo(
    bytes: &[u8],
    progress_cb: Option<js_sys::Function>,
) -> Result<JsValue, JsValue> {
    let analysis = Analysis::try_from_bytes_with_progress(bytes, |processed, total| {
        if let Some(cb) = &progress_cb {
            let _ = cb.call2(
                &JsValue::NULL,
                &JsValue::from(processed as u32),
                &JsValue::from(total as u32),
            );
        }
    })
    .map_err(|e| JsValue::from_str(&e))?;

    // `HashMap` fields (e.g. Player::weapon_breakdown) must serialize as
    // plain JS objects, not JS `Map`s — serde-wasm-bindgen's default would
    // otherwise silently produce empty results from JS's `Object.entries()`,
    // and this needs to match desktop's serde_json-over-Tauri-IPC shape.
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    analysis
        .serialize(&serializer)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}
