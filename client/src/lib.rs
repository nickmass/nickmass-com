use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, EventTarget};

mod bouncing;
mod gl;
mod header;
mod shaders;
mod youtube;

#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    ConsoleLogger::initialize();

    let window = web_sys::window().expect("unable to get window");
    let document = window.document().expect("unable to get document");

    let ready_state = document.ready_state();

    if ready_state != "loading" {
        run(&document);
    } else {
        let loaded_cb = Closure::once_into_js({
            let document = document.clone();
            move || {
                run(&document);
            }
        });

        let event_target: &EventTarget = document.as_ref();
        event_target.add_event_listener_with_callback(
            "DOMContentLoaded",
            loaded_cb.as_ref().unchecked_ref(),
        )?;
    }

    Ok(())
}

fn run(document: &Document) {
    youtube::YoutubeEmbed::attach(document);
    header::create_header(document);
}

struct ConsoleLogger;

impl ConsoleLogger {
    pub fn initialize() {
        let _ = log::set_logger(&ConsoleLogger).unwrap();
        log::set_max_level(log::LevelFilter::max());
    }
}

impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let level = metadata.level();
        cfg!(debug_assertions) || (level == log::Level::Error) || (level == log::Level::Warn)
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = record.level();
        let msg = JsValue::from_str(&format!("{}", record.args()));
        match level {
            log::Level::Error => web_sys::console::error_1(&msg),
            log::Level::Warn => web_sys::console::warn_1(&msg),
            log::Level::Info => web_sys::console::info_1(&msg),
            log::Level::Debug | log::Level::Trace => web_sys::console::debug_1(&msg),
        }
    }

    fn flush(&self) {}
}
