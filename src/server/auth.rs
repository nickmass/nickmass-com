use serde::{Deserialize, Serialize};

use super::users::User;

#[derive(Debug, Serialize, Deserialize)]
pub struct OauthResponse {
    pub state: String,
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct OauthTokenRequest<'a> {
    pub code: &'a str,
    pub client_id: &'a str,
    pub client_secret: &'a str,
    pub redirect_uri: &'a str,
    pub grant_type: &'a str,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OauthTokenResponse {
    pub access_token: String,
    #[serde(deserialize_with = "GoogleToken::deser")]
    pub id_token: GoogleToken,
    pub expires_in: u64,
    pub token_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GoogleToken {
    pub header: GoogleTokenHeader,
    pub claims: GoogleTokenClaims,
}

impl GoogleToken {
    fn deser<'d, D: serde::Deserializer<'d>>(de: D) -> Result<GoogleToken, D::Error> {
        let base64 = String::deserialize(de)?;
        let token = jwt::Token::parse_unverified(base64.as_str())
            .map_err(|e| serde::de::Error::custom(format!("{:?}", e)))?;
        let (header, claims) = token.into();
        Ok(GoogleToken { header, claims })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GoogleTokenHeader {
    pub alg: String,
    pub kid: String,
    pub typ: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GoogleTokenClaims {
    pub iss: String,
    pub sub: String,
    pub email: String,
    pub name: String,
}

pub struct Authenticated<T> {
    user: User,
    resource: T,
}

impl<T> Authenticated<T> {
    pub fn new(user: User, resource: T) -> Authenticated<T> {
        Authenticated { user, resource }
    }

    pub fn user(&self) -> &User {
        &self.user
    }
}

impl<T> std::ops::Deref for Authenticated<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.resource
    }
}

impl<T> std::ops::DerefMut for Authenticated<T> {
    //type Target = T;
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.resource
    }
}
