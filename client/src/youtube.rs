use gloo_events::{EventListener, EventListenerOptions};
use js_sys::Error;
use wasm_bindgen::JsValue;
use web_sys::{Document, Element, Event};

pub struct YoutubeEmbed;

impl YoutubeEmbed {
    pub fn attach(document: &Document) {
        let targets = document.get_elements_by_class_name("youtube-link");
        for idx in 0..targets.length() {
            if let Some(target) = targets.item(idx) {
                let options = EventListenerOptions::enable_prevent_default();
                let listener =
                    EventListener::once_with_options(target.as_ref(), "click", options, {
                        let document = document.clone();
                        let target = target.clone();
                        move |event: &Event| match youtube_click(&target, &document) {
                            Ok(_) => event.prevent_default(),
                            Err(e) => log::error!("Youtube Embed Error: {:?}", e),
                        }
                    });

                listener.forget();
            }
        }
    }
}

fn youtube_click(target: &Element, document: &Document) -> Result<(), JsValue> {
    if let Some(youtube_id) = target.get_attribute("data-video-id") {
        let player = document.create_element("iframe")?;
        player.set_attribute("width", "100%")?;
        player.set_attribute("height", "100%")?;
        player.set_attribute(
            "src",
            &format!(
                "https://www.youtube.com/embed/{}?autoplay=1&rel=0",
                youtube_id
            ),
        )?;
        player.set_attribute("allowfullscreen", "")?;
        player.set_attribute("frameborder", "0")?;
        target.replace_with_with_node_1(player.as_ref())
    } else {
        Err(Error::new("data-video-id not set").into())
    }
}
