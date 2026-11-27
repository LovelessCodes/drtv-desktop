//! The DR session: what replaces `plx_source::source::session` + `grant` + `pins` + `probe`.
//!
//! Plex needs a server list, per-server tokens, grants, probe verdicts and per-profile pins. DR
//! needs exactly one thing: a profile token, obtained once by a device code the user approves on
//! their phone, and refreshed silently afterwards.
//!
//! ## The flow (verified against the live service)
//!
//! 1. `POST https://login.dr.dk/oidc/device/authorize` with `client_id=65c09e7c9e45093875c29879`
//!    → `user_code`, `verification_uri` (`https://www.dr.dk/aktiver`), `device_code`, `interval`.
//!    The UI shows the code, the user approves it.
//! 2. poll `POST https://login.dr.dk/oidc/token` with
//!    `grant_type=urn:ietf:params:oauth:grant-type:device_code` until it returns
//!    `access_token` + `id_token` + `refresh_token`.
//! 3. `POST {API}/authorization/exchange` with `{accessToken, identityToken, scopes, device}`
//!    → an **array** of scoped tokens (Catalog/Commerce/Settings/Playback × UserAccount/UserProfile).
//! 4. renew with `POST {API}/authorization/refresh {token}` → `{value, type, expirationDate}`, or
//!    re-exchange from the OIDC `refresh_token` when the DR tokens have fully lapsed.
//!
//! ## What the rest of the app gets
//!
//! [`Tokens::profile`] is the bearer the catalog calls use; [`Tokens::session_id`] is the `sid`
//! claim that `GET /v2/search` requires (with `include`, `group` and `max_results`; it accepts a
//! `sessionId` **or** a `userId`, never both).

use serde::Deserialize;

/// One scoped token as DR sends it. **Two spellings, one meaning**: `/authorization/exchange`
/// answers `{Token, TokenType}`, while `/authorization/profile` (the seat) and
/// `/authorization/refresh` answer `{value, type, expirationDate, refreshable}`. The aliases keep
/// that a property of the wire rather than of each caller.
#[derive(Debug, Clone, Deserialize)]
struct WireToken {
    #[serde(rename = "Token", alias = "token", alias = "value", default)]
    token: String,
    #[serde(rename = "TokenType", alias = "kind", alias = "type", default)]
    kind: String,
}

/// The JWT claims this client cares about. DR puts the session id in `sid`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Claims {
    #[serde(default)]
    pub sub: String,
    #[serde(default)]
    pub sid: String,
    #[serde(rename = "userAccountId", default)]
    pub user_account_id: String,
    #[serde(rename = "userProfileId", default)]
    pub user_profile_id: String,
    #[serde(default)]
    pub exp: i64,
}

#[derive(Debug, Clone, Default)]
pub struct Tokens {
    /// `UserAccount` scoped to `Catalog` — account-level reads.
    pub account: String,
    /// `UserProfile` scoped to `Catalog` — what catalog and search calls carry.
    pub profile: String,
    /// `sid` from either token.
    pub session_id: String,
    /// `userAccountId`.
    pub user_id: String,
    /// Unix seconds from the `exp` claim.
    pub expires_at: i64,
}

impl Tokens {
    pub fn bearer(&self) -> String {
        format!("Bearer {}", self.profile)
    }
    /// DR tokens live 12 h; renew a little before that.
    pub fn needs_refresh(&self, now_s: i64) -> bool {
        self.expires_at > 0 && now_s + 1800 >= self.expires_at
    }
}

fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        })
    }
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in input.bytes() {
        if c == b'=' {
            break;
        }
        let v = val(c)?;
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Decode the claims of a DR JWT. Returns `None` for anything unparseable — a malformed token is a
/// logged-in-as-nobody, not a crash.
pub fn jwt_claims(token: &str) -> Option<Claims> {
    let payload = token.split('.').nth(1)?;
    let bytes = b64url_decode(payload)?;
    serde_json::from_slice(&bytes).ok()
}

/// Parse the `POST /authorization/exchange` response: an array of scoped tokens, not a single one.
pub fn from_exchange(json: &str) -> Option<Tokens> {
    let list: Vec<WireToken> = serde_json::from_str(json).ok()?;
    let mut out = Tokens::default();
    for t in &list {
        let claims = jwt_claims(&t.token)?;
        let catalog = claims.sub == "Catalog";
        match t.kind.as_str() {
            "UserAccount" if catalog => out.account = t.token.clone(),
            "UserProfile" if catalog => out.profile = t.token.clone(),
            _ => continue,
        }
        if out.session_id.is_empty() {
            out.session_id = claims.sid.clone();
        }
        if out.user_id.is_empty() {
            out.user_id = if claims.user_account_id.is_empty() {
                claims.user_profile_id.clone()
            } else {
                claims.user_account_id.clone()
            };
        }
        out.expires_at = out.expires_at.max(claims.exp);
    }
    if out.profile.is_empty() && out.account.is_empty() {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64url_encode(input: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in input.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(T[(n >> 18) as usize & 63] as char);
            out.push(T[(n >> 12) as usize & 63] as char);
            if chunk.len() > 1 {
                out.push(T[(n >> 6) as usize & 63] as char);
            }
            if chunk.len() > 2 {
                out.push(T[n as usize & 63] as char);
            }
        }
        out
    }

    fn jwt(claims: &str) -> String {
        // header.payload.signature — only the payload is ever read
        format!("h.{}.s", b64url_encode(claims.as_bytes()))
    }

    #[test]
    fn base64url_roundtrip() {
        // "hello" in base64url
        assert_eq!(b64url_decode("aGVsbG8").unwrap(), b"hello".to_vec());
    }

    #[test]
    fn exchange_picks_the_catalog_pair() {
        let account =
            jwt(r#"{"sub":"Catalog","sid":"SID-1","userAccountId":"ACC","exp":1792700000}"#);
        let profile =
            jwt(r#"{"sub":"Catalog","sid":"SID-1","userProfileId":"PROF","exp":1792700000}"#);
        let commerce = jwt(r#"{"sub":"Commerce","sid":"SID-1","exp":1792700000}"#);
        let json = format!(
            r#"[{{"Token":"{commerce}","TokenType":"UserAccount"}},
                {{"Token":"{account}","TokenType":"UserAccount"}},
                {{"Token":"{profile}","TokenType":"UserProfile"}}]"#
        );
        let t = from_exchange(&json).expect("tokens");
        assert_eq!(t.account, account);
        assert_eq!(t.profile, profile);
        assert_eq!(t.session_id, "SID-1");
        assert_eq!(t.user_id, "ACC");
        assert!(t.bearer().starts_with("Bearer "));
        // 12 h tokens: not due an hour before expiry, due ten minutes before it
        assert!(!t.needs_refresh(1_792_696_400));
        assert!(t.needs_refresh(1_792_699_400));
    }

    #[test]
    fn the_seat_envelope_spells_the_same_two_things_differently() {
        // `POST /authorization/profile` answers `[{value, type, expirationDate, refreshable}]`.
        // Read only the exchange spelling and a seat that DR accepted still parses to nothing.
        let profile =
            jwt(r#"{"sub":"Catalog","sid":"SID-2","userProfileId":"PROF","exp":1792700000}"#);
        let json = format!(
            r#"[{{"value":"{profile}","type":"UserProfile","refreshable":true,
                 "expirationDate":"2026-11-06T22:00:00Z"}}]"#
        );
        let t = from_exchange(&json).expect("the seat's tokens");
        assert_eq!(t.profile, profile);
        assert_eq!(t.session_id, "SID-2");
        assert_eq!(t.user_id, "PROF");
        assert!(t.account.is_empty(), "a seat mints a profile token only");
    }
}
