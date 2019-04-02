use askama::Template;
use futures::{future, Future};

use super::db::Connection;
use super::model::*;
use super::posts::{Error, PostClient};
use super::users::User;

const PAGE_SIZE: i64 = 10;

pub fn index(
    _user: Option<User>,
    db: Connection,
    page: Option<i64>,
) -> impl Future<Item = String, Error = Error> {
    let post_client = PostClient::new(db);
    let page = page.unwrap_or(1);
    let current_page = if page == 0 { 1 } else { page };
    post_client
        .get_all(PAGE_SIZE, (current_page - 1) * PAGE_SIZE)
        .map(move |page| {
            let model = PostIndex { page, current_page };
            model.render().expect("Successful Render")
        })
}

pub fn post_id(
    _user: Option<User>,
    db: Connection,
    post: u64,
) -> impl Future<Item = String, Error = Error> {
    let post_client = PostClient::new(db);
    post_client.get(post).map(|post| {
        let model = PostView { post };
        model.render().expect("Successful Render")
    })
}

pub fn post_frag(
    _user: Option<User>,
    db: Connection,
    frag: impl AsRef<str>,
) -> impl Future<Item = String, Error = Error> {
    let post_client = PostClient::new(db);
    let frag = frag.as_ref().to_string();
    post_client.get_by_fragment(frag).map(|post| {
        let model = PostView { post };
        model.render().expect("Successful Render")
    })
}

pub fn not_found(
    _user: Option<User>,
    _db: Connection,
) -> impl Future<Item = String, Error = Error> {
    future::ok(NotFound.render().expect("Successful Render"))
}
