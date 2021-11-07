use ring::aead;
use ring::rand::{SecureRandom, SystemRandom};

use super::db::Connection;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

pub struct Session {
    rand: SystemRandom,
    key: aead::LessSafeKey,
}

impl Session {
    pub fn new(session_key: impl AsRef<[u8]>) -> Session {
        let rand = SystemRandom::new();
        let key = aead::UnboundKey::new(&aead::AES_256_GCM, session_key.as_ref())
            .expect("Valid session key");
        let key = aead::LessSafeKey::new(key);
        Session { rand, key }
    }

    pub async fn get_store(
        &self,
        db: &mut Connection,
        addr: IpAddr,
        sid: Option<impl AsRef<str>>,
    ) -> Store {
        if let Some((sid, key)) =
            sid.and_then(|sid| self.decode_sid(addr, &sid).map(|key| (sid, key)))
        {
            let session_key = format!("session:{}", key);
            let store = redis::cmd("hgetall").arg(session_key).query_async(db).await;

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

    pub async fn set_store(&self, db: &mut Connection, store: Store) {
        let mut pipe = redis::pipe();
        let session_key = format!("session:{}", store.key);
        pipe.hset_multiple(session_key.as_str(), store.values().as_slice());
        pipe.expire(session_key.as_str(), 60 * 60 * 24 * 90);
        let _: Result<(), _> = pipe.query_async(db).await;
    }

    fn decode_sid(&self, addr: IpAddr, sid: impl AsRef<str>) -> Option<String> {
        let sid = sid.as_ref();
        let (nounce_str, sid) = sid.split_once('.')?;

        let mut sid_bytes = base64::decode(sid).ok()?;

        let nonce_bytes = base64::decode(nounce_str).ok()?.try_into().ok()?;
        let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);

        let sid_bytes = self
            .key
            .open_in_place(nonce, aead::Aad::empty(), &mut sid_bytes)
            .ok()?;

        let sid_string = String::from_utf8(sid_bytes.to_vec()).ok()?;
        let (user_key, ip) = sid_string.split_once('.')?;

        if ip == addr.to_string() {
            Some(user_key.to_string())
        } else {
            None
        }
    }

    fn create_key(&self) -> String {
        let mut user_key = [0; 32];
        self.rand
            .fill(&mut user_key)
            .expect("Crypto error, could not fill session user key random");
        base64::encode(&user_key[..])
    }

    pub fn create_nounce(&self) -> String {
        let mut nonce_bytes = [0; 12];
        self.rand
            .fill(&mut nonce_bytes)
            .expect("Crypto error, could not fill session nonce random");
        base64::encode(&nonce_bytes[..])
    }

    fn create_sid(&self, user_key: impl AsRef<str>, addr: IpAddr) -> String {
        use std::io::Write;

        let mut nonce_bytes = [0; aead::NONCE_LEN];
        self.rand
            .fill(&mut nonce_bytes)
            .expect("Crypto error, could not fill sid nonce");
        let nonce_str = base64::encode(&nonce_bytes);
        let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);

        let mut sid: Vec<u8> = Vec::new();
        let _ = write!(&mut sid, "{}.{}", user_key.as_ref(), addr);

        self.key
            .seal_in_place_append_tag(nonce, aead::Aad::empty(), &mut sid)
            .expect("Crypto error, failed to encrypt");

        format!("{}.{}", nonce_str, base64::encode(&sid))
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
