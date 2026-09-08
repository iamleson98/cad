//! Browser-only helpers for the wasm build (W-10).
//!
//! Downloads replace file writes: exports and `.forgecad` saves hand the
//! user a browser download instead of touching a (non-existent) disk.
//! The blob-URL + anchor technique works in every modern browser and
//! needs no user permission prompt.

use wasm_bindgen::JsCast;

/// MIME types per export format (browser download metadata).
pub fn mime_for(extension: &str) -> &'static str {
    match extension {
        "stl" => "model/stl",
        "obj" => "text/plain",
        "gltf" => "model/gltf+json",
        "3mf" => "model/3mf",
        "forgecad" => "text/plain",
        _ => "application/octet-stream",
    }
}

/// Trigger a browser download of `data` as `filename`.
///
/// Creates an object URL for the bytes, clicks a temporary `<a download>`
/// element and revokes the URL immediately (the click is synchronous —
/// the browser keeps an internal reference while the download runs).
pub fn download_bytes(filename: &str, data: Vec<u8>, mime: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or("no browser window")?;
    let document = window.document().ok_or("no document")?;

    let options = web_sys::BlobPropertyBag::new();
    options.set_type(mime);
    let parts =
        js_sys::Array::from_iter(std::iter::once(js_sys::Uint8Array::from(data.as_slice())));
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(parts.as_ref(), &options)
        .map_err(|e| format!("blob creation failed: {e:?}"))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|e| format!("object URL failed: {e:?}"))?;

    let result = (|| -> Result<(), String> {
        let anchor = document
            .create_element("a")
            .map_err(|e| format!("anchor creation failed: {e:?}"))?;
        let anchor = anchor
            .dyn_into::<web_sys::HtmlAnchorElement>()
            .map_err(|_| "anchor element cast failed".to_string())?;
        anchor.set_href(&url);
        anchor.set_download(filename);
        anchor.click();
        Ok(())
    })();

    let _ = web_sys::Url::revoke_object_url(&url);
    result
}
