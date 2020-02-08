use askama::Template;

use super::posts::{Post, PostPage};
use super::users::User;

#[derive(Template)]
#[template(path = "post_index.html")]
pub struct PostIndex {
    pub page: PostPage,
    pub current_page: i64,
    pub user: Option<User>,
}

#[derive(Template)]
#[template(path = "post_view.html")]
pub struct PostView {
    pub post: Post,
    pub user: Option<User>,
}

impl Post {
    fn render_content(&self) -> String {
        let mut output = String::new();
        let parser = pulldown_cmark::Parser::new(&self.content).map(cmark_ext_map);
        pulldown_cmark::html::push_html(&mut output, parser);

        output
    }

    fn render_date(&self) -> String {
        use chrono::*;
        let tz = FixedOffset::west(6 * 3600);
        let date_sec = (self.date / 1000) as i64;
        let date_nano = (self.date % 1000 * 1000) as u32;
        let date = NaiveDateTime::from_timestamp(date_sec, date_nano);
        let date = tz.from_utc_datetime(&date);
        date.format("%A, %B %-d, %-Y").to_string()
    }
}

use pulldown_cmark::*;

fn cmark_ext_map<'a>(item: Event) -> Event {
    match item {
        Event::Html(ref html) => {
            let matches: Vec<_> = html
                .as_ref()
                .match_indices("<youtube:")
                .map(|(idx, _)| idx)
                .collect();
            if matches.len() > 0 {
                let mut chars = html.as_ref().chars().enumerate();
                let mut new_html = String::new();
                while let Some((idx, c)) = chars.next() {
                    if matches.contains(&idx) {
                        let mut start = false;
                        let mut video_id = String::new();
                        while let Some((_, c)) = chars.next() {
                            match c {
                                ':' => start = true,
                                '>' => break,
                                _ => {
                                    if start {
                                        video_id.push(c);
                                    }
                                }
                            }
                        }
                        let embed = format!(
                            r#"
<div class="youtube-container">
    <a class="youtube-link" href="https://www.youtube.com/watch?v={video_id}" target="_blank" rel="noopener noreferrer" data-video-id="{video_id}">
        <img src="https://img.youtube.com/vi/{video_id}/hqdefault.jpg" alt="YouTube embedded video">
        <div class="youtube-play-button"></div>
    </a>
</div>"#,
                            video_id = video_id
                        );
                        new_html.push_str(&embed);
                    } else {
                        new_html.push(c);
                    }
                }

                Event::Html(new_html.into())
            } else {
                item
            }
        }
        _ => item,
    }
}

#[derive(Template)]
#[template(path = "not_found.html")]
pub struct NotFound {
    pub user: Option<User>,
}
