//! Tokens signed with the session key: `base64(json) . base64(hmac)`.
//!
//! Every token carries a `kind` and an `exp` unix timestamp, so one kind can
//! never be accepted as another (an OAuth access token as a session, say).
//! Rotating `session-key` invalidates all of them at once.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Claims {
    /// The dashboard sign-in cookie.
    Session {
        /// The user's Google Health user ID.
        uid: String,
    },
    /// A Google sign-in in progress (cookie between redirect and callback).
    Pending {
        state: String,
        verifier: String,
        /// Local path to return to after a login.
        next: String,
    },
    /// An MCP client registration; the token is its `client_id`.
    Client {
        redirect_uris: Vec<String>,
        name: String,
        auth: ClientAuth,
    },
    /// A validated authorization request, carried by the consent form.
    Consent {
        client_id: String,
        redirect_uri: String,
        state: Option<String>,
        challenge: String,
        /// As in `Session`.
        uid: String,
    },
    /// An authorization code. Stateless, so it can be redeemed more than once
    /// until it expires (OAuth 2.1 asks for single use); PKCE still binds it
    /// to the client that started the flow, and it lives only 5 minutes.
    Code {
        client_id: String,
        redirect_uri: String,
        challenge: String,
        /// As in `Session`.
        uid: String,
    },
    Access {
        /// As in `Session`.
        uid: String,
        aud: String,
    },
    Refresh {
        client_id: String,
        /// As in `Session`.
        uid: String,
    },
}

/// How an MCP client authenticates at the token endpoint (RFC 7591).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientAuth {
    None,
    ClientSecretPost,
    ClientSecretBasic,
}

impl ClientAuth {
    pub const ALL: [Self; 3] = [Self::None, Self::ClientSecretPost, Self::ClientSecretBasic];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ClientSecretPost => "client_secret_post",
            Self::ClientSecretBasic => "client_secret_basic",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == s)
    }
}

#[derive(Serialize, Deserialize)]
struct Signed {
    #[serde(flatten)]
    claims: Claims,
    exp: i64,
}

pub fn sign(key: &[u8], claims: Claims, exp: i64) -> String {
    let json = serde_json::to_string(&Signed { claims, exp }).expect("claims serialize");
    let body = URL_SAFE_NO_PAD.encode(json);
    format!("{body}.{}", URL_SAFE_NO_PAD.encode(mac(key, &body)))
}

/// The claims of a token signed with `key` that has not expired yet.
pub fn verify(key: &[u8], token: &str) -> Option<Claims> {
    let (body, sig) = token.split_once('.')?;
    let sig = URL_SAFE_NO_PAD.decode(sig).ok()?;
    let mut m = Hmac::<Sha256>::new_from_slice(key).ok()?;
    m.update(body.as_bytes());
    m.verify_slice(&sig).ok()?;
    let json = URL_SAFE_NO_PAD.decode(body).ok()?;
    let s: Signed = serde_json::from_slice(&json).ok()?;
    (s.exp > chrono::Utc::now().timestamp()).then_some(s.claims)
}

fn mac(key: &[u8], body: &str) -> Vec<u8> {
    let mut m = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    m.update(body.as_bytes());
    m.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn in_a_minute() -> i64 {
        chrono::Utc::now().timestamp() + 60
    }

    /// Signs arbitrary JSON, as tokens issued by older versions look.
    fn sign_raw(key: &[u8], v: &Value) -> String {
        let body = URL_SAFE_NO_PAD.encode(v.to_string());
        format!("{body}.{}", URL_SAFE_NO_PAD.encode(mac(key, &body)))
    }

    #[test]
    fn round_trips_and_rejects_tampering() {
        let key = b"k";
        let session = Claims::Session { uid: "1".into() };
        let t = sign(key, session.clone(), in_a_minute());
        assert_eq!(verify(key, &t), Some(session));
        assert!(verify(b"other", &t).is_none());
        let (body, sig) = t.split_once('.').unwrap();
        let forged = URL_SAFE_NO_PAD
            .encode(json!({"kind": "session", "uid": "2", "exp": in_a_minute()}).to_string());
        assert!(verify(key, &format!("{forged}.{sig}")).is_none());
        assert!(verify(key, body).is_none());
        let expired = sign(key, Claims::Session { uid: "1".into() }, 1);
        assert!(verify(key, &expired).is_none());
    }

    #[test]
    fn accepts_client_ids_issued_by_older_versions() {
        let key = b"k";
        let exp = in_a_minute();
        // Client IDs live for years in Claude's and ChatGPT's connector settings.
        let client = sign_raw(
            key,
            &json!({"kind": "client", "redirect_uris": ["https://claude.ai/cb"], "name": "Claude",
                    "auth": "none", "exp": exp}),
        );
        assert_eq!(
            verify(key, &client),
            Some(Claims::Client {
                redirect_uris: vec!["https://claude.ai/cb".into()],
                name: "Claude".into(),
                auth: ClientAuth::None,
            })
        );
    }

    #[test]
    fn rejects_tokens_without_a_user_id() {
        // Issued while users signed in with their e-mail; they sign in again.
        let key = b"k";
        let exp = in_a_minute();
        for v in [
            json!({"email": "a@b.c", "exp": exp}),
            json!({"kind": "session", "email": "a@b.c", "exp": exp}),
            json!({"kind": "refresh", "client_id": "c", "email": "a@b.c", "exp": exp}),
        ] {
            assert!(verify(key, &sign_raw(key, &v)).is_none(), "{v}");
        }
    }

    #[test]
    fn untyped_tokens_are_not_sessions() {
        let key = b"k";
        let exp = in_a_minute();
        for v in [
            json!({"email": "a@b.c", "exp": exp, "aud": "x"}),
            json!({"state": "s", "purpose": "login", "exp": exp}),
            json!({"kind": "unknown", "email": "a@b.c", "exp": exp}),
            json!({"email": "a@b.c"}),
        ] {
            assert!(verify(key, &sign_raw(key, &v)).is_none(), "{v}");
        }
    }

    #[test]
    fn client_auth_names_match_serde() {
        for a in ClientAuth::ALL {
            assert_eq!(serde_json::to_value(a).unwrap(), a.as_str());
            assert_eq!(ClientAuth::parse(a.as_str()), Some(a));
        }
    }
}
