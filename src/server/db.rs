use futures::TryFutureExt;
use redis::aio::MultiplexedConnection;
use redis::{Client, IntoConnectionInfo};
use tokio::sync::RwLock;
use tokio::time::timeout;

use std::sync::Arc;
use std::time::Duration;

use super::Error;

pub type Connection = MultiplexedConnection;

#[derive(Clone)]
pub struct Db {
    client: Arc<Client>,
    connection: Arc<RwLock<Option<Connection>>>,
}

impl Db {
    pub fn new(params: impl IntoConnectionInfo) -> Result<Db, Error> {
        Ok(Db {
            client: Arc::new(Client::open(params)?),
            connection: Arc::new(RwLock::new(None)),
        })
    }
    pub async fn get(self) -> Result<Connection, Error> {
        let existing_conn = {
            let conn = self.connection.read().await;
            (*conn).clone()
        };

        if let Some(mut conn) = existing_conn {
            let try_ping: Result<(), _> = timeout(
                Duration::from_secs(2),
                redis::cmd("PING")
                    .query_async(&mut conn)
                    .map_err(Error::from),
            )
            .await
            .unwrap_or_else(|e| Err(Error::from(e)));
            match try_ping {
                Ok(_) => Ok(conn),
                Err(err) => {
                    log::error!("Dropping redis connection: {:?}", err);
                    let mut conn = self.connection.write().await;
                    *conn = None;
                    Err(err.into())
                }
            }
        } else {
            log::info!("Creating redis connection");
            let redis = self.client.get_multiplexed_tokio_connection().await?;
            let mut conn = self.connection.write().await;
            *conn = Some(redis.clone());
            Ok(redis)
        }
    }
}

use warp::{self, Filter};

pub fn with_db(
    conn_string: impl Into<String>,
) -> Result<impl Filter<Extract = (Connection,), Error = warp::reject::Rejection> + Clone, Error> {
    let conn_string = conn_string.into();
    let db = Db::new(conn_string.as_ref())?;

    Ok(warp::any()
        .map(move || db.clone())
        .and_then(|db: Db| async { db.get().await.map_err(warp::reject::custom) }))
}
