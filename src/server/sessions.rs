use ring::aead::BoundKey;
use ring::{aead, rand};

use super::db::Connection;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

pub struct Session {
    db: Connection,
    rand: rand::SystemRandom,
    sealing_key: aead::SealingKey<CountingNonce>,
    opening_key: aead::OpeningKey<CountingNonce>,
}

struct CountingNonce {
    val: u64,
}

impl CountingNonce {
    fn new() -> Self {
        CountingNonce { val: 0 }
    }
}

impl aead::NonceSequence for CountingNonce {
    fn advance(&mut self) -> Result<aead::Nonce, ring::error::Unspecified> {
        if self.val == std::u64::MAX {
            Err(ring::error::Unspecified)
        } else {
            self.val += 1;
            let bytes = self.val.to_ne_bytes();
            let bytes = [
                0, 0, 0, 0, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                bytes[7],
            ];
            Ok(aead::Nonce::assume_unique_for_key(bytes))
        }
    }
}

impl Session {
    pub fn new(db: Connection, session_key: impl AsRef<[u8]>) -> Session {
        let sealing_key = aead::UnboundKey::new(&aead::AES_256_GCM, session_key.as_ref())
            .expect("Valid session key");
        let sealing_key = aead::SealingKey::new(sealing_key, CountingNonce::new());
        let opening_key = aead::UnboundKey::new(&aead::AES_256_GCM, session_key.as_ref())
            .expect("Valid session key");
        let opening_key = aead::OpeningKey::new(opening_key, CountingNonce::new());
        Session {
            db,
            rand: rand::SystemRandom::new(),
            sealing_key,
            opening_key,
        }
    }

    pub async fn get_store(mut self, addr: IpAddr, sid: Option<impl AsRef<str>>) -> Store {
        if let Some((sid, key)) =
            sid.and_then(|sid| self.decode_sid(addr, &sid).map(|key| (sid, key)))
        {
            let session_key = format!("session:{}", key);
            let store = redis::cmd("hgetall")
                .arg(session_key)
                .query_async(&mut self.db)
                .await;

            let store = match store {
                Ok(hash) => Store::new(key, sid.as_ref(), hash),
                Err(_) => Store::empty(key, sid.as_ref()),
            };
            store
        } else {
            let key = self.create_key();
            let sid = self.create_sid(&key, addr);
            Store::empty(key, sid)
        }
    }

    pub async fn set_store(mut self, store: Store) {
        let mut pipe = redis::pipe();
        let session_key = format!("session:{}", store.key);
        pipe.hset_multiple(session_key.as_str(), store.values().as_slice());
        pipe.expire(session_key.as_str(), 60 * 60 * 24 * 90);
        let _: Result<(), _> = pipe.query_async(&mut self.db).await;
    }

    fn decode_sid(&mut self, addr: IpAddr, sid: impl AsRef<str>) -> Option<String> {
        let mut sid_bytes = base64::decode(sid.as_ref()).ok()?;

        let sid_bytes = self
            .opening_key
            .open_in_place(aead::Aad::empty(), &mut sid_bytes)
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
        let mut user_key = [0; 32];
        self.rand
            .fill(&mut user_key)
            .expect("Crypto error, could not fill session user key random");
        base64::encode(&user_key[..])
    }

    pub fn create_nounce(&self) -> String {
        use ring::rand::SecureRandom;
        let mut nonce_bytes = [0; 12];
        self.rand
            .fill(&mut nonce_bytes)
            .expect("Crypto error, could not fill session nonce random");
        base64::encode(&nonce_bytes[..])
    }

    fn create_sid(&mut self, user_key: impl AsRef<str>, addr: IpAddr) -> String {
        let mut sid: Vec<u8> = format!("{}.{}", user_key.as_ref(), addr).as_bytes().into();

        self.sealing_key
            .seal_in_place_append_tag(aead::Aad::empty(), &mut sid)
            .expect("Crypto error, failed to encrypt");

        base64::encode(&sid)
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
