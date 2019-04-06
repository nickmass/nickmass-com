use futures::{future, Future};
use redis::r#async::SharedConnection;
use redis::{Client, IntoConnectionInfo};
use std::sync::{Arc, RwLock};

pub type Connection = SharedConnection;

pub struct Db {
    client: Client,
    connection: Arc<RwLock<Option<Connection>>>,
}

impl Db {
    pub fn new(params: impl IntoConnectionInfo) -> Result<Db, redis::RedisError> {
        Ok(Db {
            client: Client::open(params)?,
            connection: Arc::new(RwLock::new(None)),
        })
    }
    pub fn get(&self) -> impl Future<Item = Connection, Error = redis::RedisError> + Send {
        let conn = self.connection.read().unwrap();

        if let Some(conn) = conn.as_ref() {
            let conn = conn.clone();
            future::Either::A(future::ok(conn))
        } else {
            drop(conn);
            log::info!("creating redis connection");
            let conn = self.connection.clone();
            let fut = self.client.get_shared_async_connection().map(move |c| {
                log::info!("redis connection established");
                let mut conn = conn.write().unwrap();
                *conn = Some(c.clone());
                c
            });
            future::Either::B(fut)
        }
    }
}
