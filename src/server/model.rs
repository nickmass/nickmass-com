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
        let parser = pulldown_cmark::Parser::new(&self.content);
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

#[derive(Template)]
#[template(path = "not_found.html")]
pub struct NotFound {
    pub user: Option<User>,
}
