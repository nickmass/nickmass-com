use futures::{future, Future};
use redis::PipelineCommands;
use ring::{aead, rand};

use super::db::Connection;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

#[allow(dead_code)]
#[derive(Debug)]
pub enum Void {}

impl std::fmt::Display for Void {
    fn fmt(&self, _: &mut std::fmt::Formatter) -> std::fmt::Result {
        unreachable!()
    }
}

impl std::error::Error for Void {}

pub struct Session {
    db: Connection,
    rand: rand::SystemRandom,
    sealing_key: aead::SealingKey,
    opening_key: aead::OpeningKey,
}

impl Session {
    pub fn new(db: Connection, session_key: impl AsRef<[u8]>) -> Session {
        let sealing_key = aead::SealingKey::new(&aead::AES_256_GCM, session_key.as_ref())
            .expect("Valid session key");
        let opening_key = aead::OpeningKey::new(&aead::AES_256_GCM, session_key.as_ref())
            .expect("Valid session key");
        Session {
            db,
            rand: rand::SystemRandom::new(),
            sealing_key,
            opening_key,
        }
    }

    pub fn get_store(
        &self,
        addr: IpAddr,
        sid: Option<impl AsRef<str>>,
    ) -> impl Future<Item = Store, Error = Void> {
        if let Some((sid, key)) =
            sid.and_then(|sid| self.decode_sid(addr, &sid).map(|key| (sid, key)))
        {
            let ok_key = key.clone();
            let ok_sid = sid.as_ref().to_string();
            let fut = redis::cmd("hgetall")
                .arg(format!("session:{}", key))
                .query_async(self.db.clone())
                .map(move |(_conn, hash)| Store::new(ok_key, ok_sid, hash))
                .or_else(move |_e| Ok(Store::empty(key, sid.as_ref())));
            future::Either::A(fut)
        } else {
            let key = self.create_key();
            let sid = self.create_sid(&key, addr);
            future::Either::B(future::ok(Store::empty(key, sid)))
        }
    }

    pub fn set_store(&self, store: Store) -> impl Future<Item = (), Error = Void> {
        let mut pipe = redis::pipe();
        let redis_key = format!("session:{}", store.key);
        pipe.hset_multiple(redis_key.as_str(), store.values().as_slice());
        pipe.expire(redis_key.as_str(), 60 * 60 * 24 * 90);
        pipe.query_async(self.db.clone())
            .map(|_: (_, ())| ())
            .or_else(|_e| Ok(()))
    }

    fn decode_sid(&self, addr: IpAddr, sid: impl AsRef<str>) -> Option<String> {
        let mut sid_bytes = base64::decode(sid.as_ref()).ok()?;

        let nonce = aead::Nonce::try_assume_unique_for_key(&sid_bytes[0..12]).ok()?;

        let sid_bytes = aead::open_in_place(
            &self.opening_key,
            nonce,
            aead::Aad::empty(),
            12,
            &mut sid_bytes,
        )
        .ok()?;
        let sid_string = String::from_utf8(sid_bytes.to_vec()).ok()?;
        let mut parts = sid_string.splitn(2, '.');
        let user_key = parts.next()?;
        let ip = parts.next()?;

        if ip == addr.to_string() {
            Some(user_key.to_string())
        } else {
            None
        }
    }

    fn create_key(&self) -> String {
        use ring::rand::SecureRandom;
        let mut user_key = [0; 128];
        self.rand
            .fill(&mut user_key)
            .expect("Crypto error, could not fill session user key random");
        base64::encode(&user_key[..])
    }

    fn create_sid(&self, user_key: impl AsRef<str>, addr: IpAddr) -> String {
        use ring::rand::SecureRandom;
        let mut sid: Vec<u8> = format!("{}.{}", user_key.as_ref(), addr).as_bytes().into();
        let mut nonce_bytes = [0; 12];
        self.rand
            .fill(&mut nonce_bytes)
            .expect("Crypto error, could not fill session nonce random");
        let nonce = aead::Nonce::try_assume_unique_for_key(&nonce_bytes[..])
            .expect("Crypto error, incorrect nonce length");

        let suffix_len = self.sealing_key.algorithm().tag_len();
        sid.resize(sid.len() + suffix_len, 0);

        let out_len = aead::seal_in_place(
            &self.sealing_key,
            nonce,
            aead::Aad::empty(),
            &mut sid,
            suffix_len,
        )
        .expect("Crypto error, failed to encrypt");

        let mut sid_bytes = Vec::from(&nonce_bytes[..]);
        sid_bytes.extend_from_slice(&sid[..out_len]);
        base64::encode(&sid_bytes)
    }
}

#[derive(Debug, Clone)]
pub struct Store {
    key: Arc<String>,
    sid: Arc<String>,
    inner: Arc<Mutex<HashMap<String, String>>>,
}

impl Store {
    fn new(key: impl Into<String>, sid: impl Into<String>, data: HashMap<String, String>) -> Store {
        Store {
            key: Arc::new(key.into()),
            sid: Arc::new(sid.into()),
            inner: Arc::new(Mutex::new(data)),
        }
    }

    fn empty(key: impl Into<String>, sid: impl Into<String>) -> Store {
        Store {
            key: Arc::new(key.into()),
            sid: Arc::new(sid.into()),
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn values(&self) -> Vec<(String, String)> {
        let hash = self.inner.lock().unwrap();
        hash.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub fn get(&self, key: impl AsRef<str>) -> Option<String> {
        self.inner.lock().unwrap().get(key.as_ref()).cloned()
    }

    pub fn set(&self, key: impl Into<String>, value: impl Into<String>) {
        self.inner.lock().unwrap().insert(key.into(), value.into());
    }

    pub fn sid(&self) -> String {
        self.sid.to_string()
    }
}
