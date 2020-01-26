use askama::Template;

use super::db::Connection;
use super::models::*;
use super::posts::PostClient;
use super::users::User;
use super::Error;

const PAGE_SIZE: i64 = 10;

pub async fn index(user: Option<User>, db: Connection, page: Option<i64>) -> Result<String, Error> {
    let post_client = PostClient::new(db);
    let page = page.unwrap_or(1);
    let current_page = if page == 0 { 1 } else { page };
    let page = post_client
        .get_all(PAGE_SIZE, (current_page - 1) * PAGE_SIZE)
        .await?;
    let model = PostIndex {
        page,
        current_page,
        user,
    };

    model.render().map_err(|e| Error::Render(("index", e)))
}

pub async fn post_id(user: Option<User>, db: Connection, post: u64) -> Result<String, Error> {
    let post_client = PostClient::new(db);
    let post = post_client.get(post).await?;
    let model = PostView { post, user };
    model.render().map_err(|e| Error::Render(("post_id", e)))
}

pub async fn post_frag(
    user: Option<User>,
    db: Connection,
    frag: impl AsRef<str>,
) -> Result<String, Error> {
    let post_client = PostClient::new(db);
    let frag = frag.as_ref().to_string();
    let post = post_client.get_by_fragment(frag).await?;
    let model = PostView { post, user };
    model.render().map_err(|e| Error::Render(("post_frag", e)))
}

pub fn not_found(user: Option<User>) -> Result<String, Error> {
    NotFound { user }
        .render()
        .map_err(|e| Error::Render(("not_found", e)))
}

pub fn error(user: Option<User>, _error: &Error) -> Result<String, Error> {
    NotFound { user }
        .render()
        .map_err(|e| Error::Render(("not_found", e)))
}
