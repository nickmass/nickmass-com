use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{window, Document, EventTarget};

mod gl;
mod header;

#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let window = window().expect("unable to get window");
    let document = window.document().expect("unable to get document");

    let event_target: &EventTarget = document.as_ref();

    let ready_state = document.ready_state();

    if ready_state != "loading" {
        run(document);
    } else {
        let document = document.clone();
        let loaded_cb = Closure::once_into_js(move || {
            run(document);
        });
        event_target.add_event_listener_with_callback(
            "DOMContentLoaded",
            loaded_cb.as_ref().unchecked_ref(),
        )?;
    }

    Ok(())
}

fn run(document: Document) {
    header::create_header(document);
}
