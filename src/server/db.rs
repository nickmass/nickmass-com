use futures::{future, Future};
use redis::r#async::SharedConnection;
use redis::{Client, IntoConnectionInfo};
use std::sync::{Arc, RwLock};

use super::Error;

pub type Connection = SharedConnection;

pub struct Db {
    client: Client,
    connection: Arc<RwLock<Option<Connection>>>,
}

impl Db {
    pub fn new(params: impl IntoConnectionInfo) -> Result<Db, Error> {
        Ok(Db {
            client: Client::open(params)?,
            connection: Arc::new(RwLock::new(None)),
        })
    }
    pub fn get(&self) -> impl Future<Item = Connection, Error = Error> + Send {
        let conn = self.connection.read().unwrap();

        if let Some(conn) = conn.as_ref() {
            let err_conn = self.connection.clone();
            let fut = redis::cmd("PING")
                .query_async(conn.clone())
                .and_then(|(conn, _): (_, ())| Ok(conn))
                .or_else(move |err| {
                    log::error!("Dropping redis connection: {:?}", err);
                    let mut conn = err_conn.write().unwrap();
                    *conn = None;
                    Err(err)
                })
                .from_err::<Error>();
            future::Either::A(fut)
        } else {
            drop(conn);
            log::info!("Creating redis connection");
            let conn = self.connection.clone();
            let fut = self
                .client
                .get_shared_async_connection()
                .from_err::<Error>()
                .map(move |c| {
                    log::info!("Redis connection established");
                    let mut conn = conn.write().unwrap();
                    *conn = Some(c.clone());
                    c
                });
            future::Either::B(fut)
        }
    }
}
