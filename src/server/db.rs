pub use deadpool_redis::Connection;

use std::sync::Arc;

use super::Error;

#[derive(Clone)]
pub struct Db {
    pool: Arc<deadpool_redis::Pool>,
}

impl Db {
    pub fn new<S: Into<String>>(url: S) -> Result<Db, Error> {
        let pool = deadpool_redis::Config::from_url(url)
            .create_pool(Some(deadpool_redis::Runtime::Tokio1))?;
        Ok(Db {
            pool: Arc::new(pool),
        })
    }

    #[tracing::instrument(name = "db::get", skip_all, err)]
    pub async fn get(&self) -> Result<Connection, Error> {
        Ok(self.pool.get().await?)
    }
}
