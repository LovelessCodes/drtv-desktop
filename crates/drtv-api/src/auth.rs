//! DR sign-in: the OIDC device-code flow, the token exchange, and where the tokens live.
//!
//! The television never sees a password. It asks `login.dr.dk` for a device code, shows the user
//! code on screen, and the viewer approves it at `dr.dk/aktiver` on a phone. The TV then polls,
//! trades the OIDC tokens for DR account tokens, and remembers the profile token.
//!
//! Every request and every response shape below was verified against the live service (see the
//! parent repository's NOTES.md, "The token model turned out to be two layers").

use serde::{Deserialize, Serialize};

use crate::api::api_base;
use crate::session::{from_exchange, Tokens};

const OIDC: &str = "https://login.dr.dk";
/// [`OIDC`] unless `DRTV_OIDC` names another server — see [`crate::base`].
fn oidc_base() -> String {
    crate::base(OIDC, "DRTV_OIDC")
}
/// The DRTV television client. A web client id is a different audience and is refused by
/// `/oidc/device/authorize` ("device_code is not allowed for this client").
const CLIENT_ID: &str = "65c09e7c9e45093875c29879";
/// Where a TELEVISION keeps the session. Persisted on the dev partition: `/tmp` is wiped on
/// reboot, and signing in again after every power cycle would make the feature useless.
pub const DEFAULT_AUTH_FILE: &str = "/media/developer/drtv-auth.json";

/// The path this process actually uses. A host build has no `/media/developer`, so without this the
/// device-code flow would complete and then fail to `save()` — the one thing that made the desktop
/// simulator unable to hold a DR session at all. The app layer sets this at boot to its instance
/// root (`plx_base::paths::in_runtime_dir`), the same tier the Plex session uses off-device
/// (`paths::session_candidates`); nothing here knows what a simulator is.
static AUTH_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Steer where the session lives. First call wins, so a boot that sets it early is authoritative
/// and no later caller can move the tokens out from under an open store.
pub fn set_auth_path(path: std::path::PathBuf) {
    let _ = AUTH_PATH.set(path);
}

fn auth_path() -> &'static std::path::Path {
    AUTH_PATH.get_or_init(|| std::path::PathBuf::from(DEFAULT_AUTH_FILE))
}

/// **Every place the session may live, in order.** One path was not enough: `/media/developer` is
/// the dev-mode area and this set came back from standby with the file gone (the Plex session has
/// three candidates for exactly this reason — its second is a different partition, `/media/internal`,
/// an ext4 mount that survives a power cycle). `auth_path` stays the FIRST candidate, so nothing
/// about the existing file moves; this only means a session written there can be found again, and a
/// save that fails there is written somewhere that lasts. A steered build (the simulator) keeps its
/// single path and is not offered anything else.
fn candidates() -> Vec<std::path::PathBuf> {
    if let Some(steered) = AUTH_PATH.get() {
        return vec![steered.clone()];
    }
    vec![
        std::path::PathBuf::from(DEFAULT_AUTH_FILE),
        std::path::PathBuf::from("/media/internal/.drtv-auth.json"),
    ]
}

fn existing() -> Option<std::path::PathBuf> {
    candidates().into_iter().find(|p| p.exists())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DeviceCode {
    #[serde(default)]
    pub device_code: String,
    #[serde(default)]
    pub user_code: String,
    #[serde(default)]
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: String,
    #[serde(default)]
    pub interval: u64,
    #[serde(default)]
    pub expires_in: u64,
}

impl DeviceCode {
    /// What to put on screen: the short URL and the code, as separate lines.
    pub fn instructions(&self) -> String {
        let url = if self.verification_uri.is_empty() {
            "dr.dk/aktiver".to_string()
        } else {
            self.verification_uri.replace("https://", "").replace("http://", "")
        };
        format!("DRTV login: {url} — kode {}", self.user_code)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TokenReply {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    id_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stored {
    /// OIDC refresh token — the long-lived half; the DR tokens can always be re-exchanged from it.
    pub oidc_refresh: String,
    pub account: String,
    pub profile: String,
    pub session_id: String,
    pub user_id: String,
    pub expires_at: i64,
    /// DR's id for the seated profile, `""` when none was chosen. Persisted under its original
    /// key; [`de_profile_id`] still reads the integer this field used to be, as nothing chosen —
    /// no real DR id is a number, and a stale one would be sent to a route that refuses it.
    #[serde(default, deserialize_with = "de_profile_id")]
    pub profile_id: String,
    /// What DR calls this account ("Loveless"), for the account menu. Empty when the profile call
    /// failed — the menu falls back to "DR".
    #[serde(default)]
    pub name: String,
    /// The seated profile's own `segments`, written when it is chosen and read back on every
    /// request that has to name a universe ([`Universe`]). Empty on a file written before this was
    /// persisted, which reads as the general catalogue — never as somebody else's.
    #[serde(default)]
    pub segments: Vec<String>,
    /// The profile this television **starts with** ([`set_auto`]), `""` when the viewer has pinned
    /// none — which is the default, and the state a fresh sign-in lands in. It lives beside the
    /// seat rather than in a settings file because it is meaningless without the account it names
    /// and must die with it: signing out deletes this record.
    #[serde(default)]
    pub auto_profile_id: String,
}

impl Stored {
    /// The catalogue universe this session may ask for. **Every reader of [`Self::segments`] goes
    /// through this**, so a request cannot name a universe the seat did not — the front page
    /// ([`crate::api::home`]) and search ([`crate::api::search`]) are the two that ask, and search
    /// used to name the general catalogue for everybody until it was made to read this.
    pub fn universe(&self) -> Universe {
        Universe::of(&self.segments)
    }
}

/// **Which catalogue a profile may see**, as DR's `segments` names it.
///
/// The roster row carries it (`drtv` for the general catalogue; `minisjang`, `ramasjang` and
/// `ultra` for the children's universes, up to about 3, 8 and 14) and the television front page is
/// composed PER UNIVERSE: `/page?segments=drtv&path=%2F` answers the adult page, while the same
/// call with `segments=minisjang` answers a ONE-ROW stub ("Gå til Minisjang") whose entry names
/// `moreLinkUrl: /minisjang` — the children's front page lives at its own path (FORK.md Step 98).
///
/// Both halves come out of the same word, so they are one value here rather than two lookups that
/// can drift: a request that names a universe with no path, or a path with no universe, is a
/// combination DR does not serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Universe {
    /// The `segments` query value, decoded (`minisjang,optedout`) — the row's own list, joined.
    pub segments: String,
    /// The front page's path: `/minisjang`, `/ramasjang`, `/ultra`, or `/` for the general
    /// catalogue.
    pub path: String,
}

impl Universe {
    /// The general (adult) catalogue: what an anonymous client asks for, and what a row that names
    /// no universe gets.
    pub fn general() -> Self {
        Self { segments: "drtv,optedout".to_string(), path: "/".to_string() }
    }

    /// The universe a roster row's `segments` names. A row names exactly one; when it names none
    /// (or only `optedout`, the consent flag every row carries) the general catalogue is the
    /// answer, because a client that guessed a children's universe for an unknown word would hide
    /// the whole catalogue behind a typo.
    pub fn of(segments: &[String]) -> Self {
        const WORLDS: [&str; 3] = ["minisjang", "ramasjang", "ultra"];
        let named: Vec<String> = segments.iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        let world = named.iter().find(|s| WORLDS.contains(&s.as_str()));
        match world {
            Some(w) => Self { segments: named.join(","), path: format!("/{w}") },
            None => Self::general(),
        }
    }
}

/// Outcome of one poll.
pub enum Poll {
    /// Not approved yet — keep waiting.
    Pending,
    /// The service asked us to back off.
    SlowDown,
    /// Approved; these are the OIDC tokens to exchange.
    Approved { access: String, id: String, refresh: String },
    /// Terminal: expired, refused, or unreachable.
    Failed(String),
}

fn form_post(url: &str, body: String) -> Option<String> {
    let headers = vec!["Content-Type: application/x-www-form-urlencoded".to_string()];
    let r = drtv_net::post(url, &headers, body.as_bytes()).ok()?;
    if !r.ok() {
        // The token endpoint answers 400 with a JSON `error`; keep the body so the caller can read
        // `authorization_pending` rather than treating a poll as a fault.
        return String::from_utf8(r.body).ok();
    }
    String::from_utf8(r.body).ok()
}

/// Step 1: ask for a device code. `None` means the service refused or the network failed.
pub fn start() -> Option<DeviceCode> {
    let body = format!("client_id={CLIENT_ID}&scope=openid%20offline_access");
    let raw = form_post(&format!("{}/oidc/device/authorize", oidc_base()), body)?;
    let code: DeviceCode = serde_json::from_str(&raw).ok()?;
    if code.device_code.is_empty() || code.user_code.is_empty() {
        return None;
    }
    Some(code)
}

/// Step 2: one poll. Call this every `DeviceCode::interval` seconds until it stops saying pending.
pub fn poll(code: &DeviceCode) -> Poll {
    let body = format!(
        "grant_type=urn:ietf:params:oauth:grant-type:device_code&device_code={}&client_id={CLIENT_ID}",
        code.device_code
    );
    let Some(raw) = form_post(&format!("{}/oidc/token", oidc_base()), body) else {
        return Poll::Failed("unreachable".into());
    };
    let reply: TokenReply = serde_json::from_str(&raw).unwrap_or_default();
    if !reply.access_token.is_empty() {
        return Poll::Approved {
            access: reply.access_token,
            id: reply.id_token,
            refresh: reply.refresh_token,
        };
    }
    match reply.error.as_str() {
        "authorization_pending" => Poll::Pending,
        "slow_down" => Poll::SlowDown,
        "" => Poll::Failed("empty reply".into()),
        other => Poll::Failed(other.to_string()),
    }
}

/// Step 3: trade the OIDC tokens for DR account tokens, and keep the pair worth persisting.
pub fn exchange(access: &str, id: &str, refresh: &str) -> Option<Stored> {
    let body = serde_json::json!({
        "accessToken": access,
        "identityToken": id,
        "scopes": ["Catalog", "Commerce", "Settings", "Playback"],
        "device": crate::api::DEVICE,
        "optout": false,
    })
    .to_string();
    let headers = vec![
        "Content-Type: application/json".to_string(),
        "Origin: https://www.dr.dk".to_string(),
    ];
    let r = drtv_net::post(
        &format!("{}/authorization/exchange", api_base()),
        &headers,
        body.as_bytes(),
    )
    .ok()?;
    if !r.ok() {
        return None;
    }
    let raw = String::from_utf8(r.body).ok()?;
    let tokens: Tokens = from_exchange(&raw)?;
    let name = profile_name(&tokens.profile).unwrap_or_default();
    Some(Stored {
        oidc_refresh: refresh.to_string(),
        account: tokens.account,
        profile: tokens.profile,
        session_id: tokens.session_id,
        user_id: tokens.user_id,
        expires_at: tokens.expires_at,
        profile_id: String::new(),
        name,
        segments: Vec::new(),
        // A fresh exchange has no pin: the caller's record is where one lives, and `renew` carries
        // it across (see the note there).
        auto_profile_id: String::new(),
    })
}

/// One viewer profile on the DR account. `GET /account` carries the roster under `profiles` — the
/// who's-watching picker draws it, and a profile token is minted for the chosen id by
/// [`select_profile`]. **The id is a string** (a UUID, or 32 hex characters on this account): an
/// earlier version of this file guessed integer ids and a `GET /account/profiles` route, which DR
/// answers `400 body.profileId is not of a type(s) string` and `405 allow: OPTIONS, POST` to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// Absolute avatar URL when DR sends one. Empty is the normal case on the TV account.
    pub avatar: String,
    pub is_default: bool,
    /// DR's own colour for this member (`#FF001E`), the fill the picker gives a tile that has no
    /// avatar to draw. Empty means DR sent none, and the picker falls back to its own surface.
    pub color: String,
    /// The profile is behind a PIN (`pinEnabled`): seating it needs the four digits.
    pub pin: bool,
    /// DR's own words for what this member may see — `["drtv","optedout"]` for the general
    /// catalogue, `["minisjang","optedout"]` / `["ramasjang","optedout"]` / `["ultra","optedout"]`
    /// for the children's universes. Carried verbatim: [`Universe`] is the only thing that reads
    /// meaning into it, and sending the row's own list back is cheaper than reconstructing it.
    pub segments: Vec<String>,
}

/// The account's viewer profiles, read with the account-level token.
///
/// DR answers either a bare array or `{profiles|items: [...]}`; the fields are tolerated rather
/// than pinned because the catalogue's own account payloads are not part of the recorded fixture
/// set (they carry personal data). `None` means the account token is missing or the request failed
/// — never "no profiles", which is an empty `Some`.
pub fn profiles() -> Option<Vec<Profile>> {
    let Some(stored) = load() else {
        crate::log("dr-auth: no session on disk, so no account token to list profiles with");
        return None;
    };
    if stored.account.is_empty() {
        crate::log("dr-auth: the session carries no account token, so a roster request would be anonymous");
        return None;
    }
    let headers = vec![
        format!("Authorization: Bearer {}", stored.account),
        format!("device: {}", crate::api::DEVICE),
    ];
    let Ok(r) = drtv_net::get(&format!("{}/account", api_base()), &headers) else {
        crate::log("dr-auth: the profile roster request never reached DR");
        return None;
    };
    if !r.ok() {
        crate::log(&format!("dr-auth: DR refused the profile roster (HTTP {})", r.status));
        return None;
    }
    let Ok(raw) = String::from_utf8(r.body) else {
        crate::log("dr-auth: the profile roster reply was not UTF-8");
        return None;
    };
    let out = parse_profiles(&raw);
    if out.is_none() {
        // **Shape, never values.** The reply is an account's own people; what a mismatched parser
        // needs is the key names, and it needs them somewhere a person can read them off a set log.
        let shape = match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(serde_json::Value::Array(rows)) => format!("an array of {}", rows.len()),
            Ok(serde_json::Value::Object(o)) => {
                format!("an object with keys {:?}", o.keys().collect::<Vec<_>>())
            }
            Ok(_) => "a scalar".to_string(),
            Err(_) => "not JSON".to_string(),
        };
        crate::log(&format!("dr-auth: the profile roster reply did not parse ({shape})"));
    }
    out
}

/// Parse `/account/profiles`. DR answers either a bare array or `{profiles|items: [...]}`, and the
/// field names are tolerated rather than pinned — the account payloads are deliberately not part
/// of the recorded fixture set. A row with no usable id is skipped; the order DR sends is kept,
/// because that is the order the picker shows.
fn parse_profiles(raw: &str) -> Option<Vec<Profile>> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let (rows, primary) = match &v {
        serde_json::Value::Array(rows) => (rows.clone(), String::new()),
        obj => {
            let primary = obj.get("primaryProfileId").map(as_id).unwrap_or_default();
            let rows = obj.get("profiles").or_else(|| obj.get("items"))
                .and_then(|p| p.as_array()).cloned()?;
            (rows, primary)
        }
    };
    let total = rows.len();
    let mut out = Vec::new();
    let mut skipped = 0usize;
    for row in rows {
        let id = ["id", "profileId"].iter()
            .find_map(|k| row.get(*k))
            .map(as_id).unwrap_or_default();
        if id.is_empty() { skipped += 1; continue; }
        let name = ["name", "title", "profileName"].iter()
            .find_map(|k| row.get(*k).and_then(|s| s.as_str()))
            .unwrap_or("DR").trim().to_string();
        let avatar = ["avatar", "image", "imageUrl", "thumb", "profileImage"].iter()
            .find_map(|k| row.get(*k).and_then(|s| s.as_str()))
            .unwrap_or("").to_string();
        // The account's own default is a TOP-LEVEL fact (`primaryProfileId`), not a flag on the
        // row: the rows carry `isActive`, which several profiles can be at once and which this
        // client must not read as "the chosen one".
        let flagged = ["isDefault", "default", "isPrimary"].iter()
            .any(|k| row.get(*k).and_then(|b| b.as_bool()).unwrap_or(false));
        let is_default = (!primary.is_empty() && id == primary) || flagged;
        let color = ["color", "colour"].iter()
            .find_map(|k| row.get(*k).and_then(|s| s.as_str()))
            .unwrap_or("").to_string();
        let pin = ["pinEnabled", "pin"].iter()
            .any(|k| row.get(*k).and_then(|b| b.as_bool()).unwrap_or(false));
        let segments = row.get("segments").and_then(|s| s.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_str()).map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()).collect())
            .unwrap_or_default();
        out.push(Profile { id, name, avatar, is_default, color, pin, segments });
    }
    if skipped > 0 {
        crate::log(&format!("dr-auth: {skipped} of {total} roster rows carried no usable id and were skipped"));
    }
    Some(out)
}

/// DR spells an id as a string. A number is tolerated because a tolerant reader is cheaper than a
/// lost roster — and because `expirationDate`-style replies have surprised this client before.
fn as_id(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Read a `profile_id` written by either side of the string change. An integer becomes empty: it
/// was this file's own invention, and no DR route accepts it.
fn de_profile_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    use serde::Deserialize as _;
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(s) => s,
        _ => String::new(),
    })
}

/// Why a seat was not established. Each variant is a different answer, and this used to be one
/// `None`: a missing account token, an unreachable network, DR's own refusal and a reply carrying
/// no catalogue token all reached the log as "DR refused the profile token", which is only ever
/// true of [`SeatRefusal::Refused`]. A wrong diagnosis here costs a session on the television.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatRefusal {
    /// No session on disk to seat anything into.
    NoSession,
    /// The stored session carries no account-level token, so the request would be anonymous.
    NoAccountToken,
    /// An empty id is not a profile — it is what a session written before profile selection
    /// carries, and what a roster row with no `id` would give.
    NoProfileId,
    /// The request never reached DR.
    Unreachable,
    /// DR answered, and not with success.
    Refused(u16),
    /// A success that carried no catalogue token: the reply's shape changed, or it was not a seat.
    NoProfileToken,
    /// The seated record could not be written, so it would not outlive the process.
    NotSaved,
}

impl SeatRefusal {
    /// The one-line account of the failure, for the event log.
    pub fn why(self) -> String {
        match self {
            Self::NoSession => "no session on disk".into(),
            Self::NoAccountToken => "the session carries no account token".into(),
            Self::NoProfileId => "no profile id was given".into(),
            Self::Unreachable => "DR was unreachable".into(),
            Self::Refused(status) => format!("DR refused the seat (HTTP {status})"),
            Self::NoProfileToken => "DR answered the seat with no catalogue token".into(),
            Self::NotSaved => "the seated record could not be saved".into(),
        }
    }
}

/// Mint a `UserProfile` catalogue token for `profile_id` and remember it. The account token and
/// the long-lived OIDC refresh token stay as they were; only the seated profile moves.
pub fn select_profile(profile_id: &str, segments: &[String]) -> Result<Stored, SeatRefusal> {
    let stored = load().ok_or(SeatRefusal::NoSession)?;
    let seated = seat_from(stored, profile_id, segments)?;
    save(&seated).then_some(seated).ok_or(SeatRefusal::NotSaved)
}

/// The network half of seating, shared by [`select_profile`] (which starts from the file) and
/// [`renew`] (which starts from the record it is about to save). One answer for both, because a
/// picker and a refresh must not disagree about what "seated" means.
fn seat_from(mut stored: Stored, profile_id: &str, segments: &[String]) -> Result<Stored, SeatRefusal> {
    if stored.account.is_empty() {
        return Err(SeatRefusal::NoAccountToken);
    }
    if profile_id.is_empty() {
        return Err(SeatRefusal::NoProfileId);
    }
    let body = serde_json::json!({
        "profileId": profile_id,
        "scopes": ["Catalog"],
    })
    .to_string();
    let headers = vec![
        "Content-Type: application/json".to_string(),
        "Origin: https://www.dr.dk".to_string(),
        format!("Authorization: Bearer {}", stored.account),
        format!("device: {}", crate::api::DEVICE),
    ];
    let r = drtv_net::post(
        &format!("{}/authorization/profile", api_base()),
        &headers,
        body.as_bytes(),
    )
    .map_err(|_| SeatRefusal::Unreachable)?;
    if !r.ok() {
        return Err(SeatRefusal::Refused(r.status));
    }
    let raw = String::from_utf8(r.body).map_err(|_| SeatRefusal::NoProfileToken)?;
    let tokens = from_exchange(&raw).ok_or(SeatRefusal::NoProfileToken)?;
    if tokens.profile.is_empty() {
        return Err(SeatRefusal::NoProfileToken);
    }
    stored.session_id = if tokens.session_id.is_empty() { stored.session_id } else { tokens.session_id };
    stored.user_id = if tokens.user_id.is_empty() { stored.user_id } else { tokens.user_id };
    stored.expires_at = tokens.expires_at;
    stored.profile = tokens.profile;
    stored.profile_id = profile_id.to_string();
    // The universe comes from the roster ROW, never from the id: a seat that named a different one
    // would show this viewer another member's catalogue while the history it writes stayed theirs.
    stored.segments = segments.to_vec();
    if let Some(name) = profile_name(&stored.profile) { stored.name = name; }
    Ok(stored)
}

/// The universe of the SEATED profile — [`Universe::general`] when nothing is seated, which is
/// also the only universe an anonymous client may ask for.
pub fn universe() -> Universe {
    load().map(|s| s.universe()).unwrap_or_else(Universe::general)
}

/// The account's display name, from DR's own profile endpoint. DR's tokens carry no name — only a
/// pseudonymous `user_id` and an `@example.com` address — so this is the one place a person is
/// called anything.
pub fn profile_name(profile_token: &str) -> Option<String> {
    let headers = vec![
        format!("Authorization: Bearer {profile_token}"),
        format!("device: {}", crate::api::DEVICE),
    ];
    let r = drtv_net::get(&format!("{}/account/profile", api_base()), &headers).ok()?;
    if !r.ok() {
        return None;
    }
    let raw = String::from_utf8(r.body).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("name")?.as_str().map(|s| s.trim().to_string())
}

/// Forget the session entirely — the account menu's "Sign out".
pub fn sign_out() -> bool {
    // Every candidate, not just the first: a session found on the fallback must not outlive the
    // sign-out that was meant to end it.
    let mut removed = false;
    for path in candidates() {
        removed |= std::fs::remove_file(path).is_ok();
    }
    removed
}

pub fn save(stored: &Stored) -> bool {
    let Ok(text) = serde_json::to_string(stored) else { return false };
    // Where it already lives, else the first candidate that takes it. A read-only or full
    // `/media/developer` (and this set has come back from standby without the file) must not cost
    // the viewer their session.
    let targets: Vec<std::path::PathBuf> = match existing() {
        Some(path) => vec![path],
        None => candidates(),
    };
    targets.into_iter().any(|path| write_owner_only(&path, &text).is_ok())
}

/// Write the session at mode `0600`, and never create it any other way.
///
/// **`std::fs::write` creates with `0666 & !umask` — 0644 here** — so on a television that is not
/// rooted, the session token was readable by every other app's uid from the instant it hit the
/// disk. `/media/developer` is shared, and this file is a live DR identity. The mode goes in
/// `open(2)`'s own argument (`OpenOptionsExt::mode`), because a chmod after the write cannot
/// un-expose a file that already existed permissively; the chmod that follows is for the file an
/// older build left behind, and it is the one case where "already exposed" is the premise rather
/// than the risk. The source layer's session writer (`source::session`) states the same rule and
/// solves it the same way, with a 0600 temp file and a rename.
fn write_owner_only(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn load() -> Option<Stored> {
    let path = existing().unwrap_or_else(|| auth_path().to_path_buf());
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Remember `profile_id` as the profile this television starts with; an empty id forgets it.
/// `false` means the record could not be read or written, so a caller that reports the outcome may
/// not claim the pin landed.
///
/// Writing the session file is what makes this survive a relaunch — the pin is a property of this
/// install's session, and a sign-out deletes the file, which is exactly right: the next account on
/// this set is not the one that was pinned.
pub fn set_auto(profile_id: &str) -> bool {
    let Some(mut stored) = load() else { return false };
    if stored.auto_profile_id == profile_id {
        return true;
    }
    stored.auto_profile_id = profile_id.to_string();
    save(&stored)
}

/// The pinned start-up profile's DR id, if the viewer set one. The one reader of
/// [`Stored::auto_profile_id`]: the boot asks it whether to raise the picker at all, and the picker
/// marks the tile it lands on.
pub fn auto_profile() -> Option<String> {
    load().map(|s| s.auto_profile_id).filter(|id| !id.is_empty())
}

/// Is there a profile token on disk? **Not the same question as "can it be used"** — an expired
/// session still answers true, which is right for `sign_out` (a file to delete is a sign-out to
/// perform) and wrong for anything that only wants to know whether the viewer is signed in.
pub fn signed_in() -> bool {
    load().is_some_and(|s| !s.profile.is_empty())
}

/// Is the stored session still worth using, without touching the network? An expired one needs the
/// refresh (`ensure_fresh`), so it is not usable AS-IS — and a caller that treats "a file exists"
/// as "somebody is signed in" will act on a session that cannot make a single request.
pub fn usable() -> bool {
    load().is_some_and(|s| !s.profile.is_empty() && !needs_refresh(&s, now()))
}

/// Refresh this long before expiry. The tokens live twelve hours; a five-minute margin means a
/// request can never be sent with a token that dies in flight.
const MARGIN: i64 = 300;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Whether the stored session has to be renewed before it can be used.
pub fn needs_refresh(stored: &Stored, at: i64) -> bool {
    stored.profile.is_empty() || stored.expires_at - MARGIN <= at
}

/// What [`ensure_fresh`] found.
pub enum Freshness {
    /// Still valid; use it.
    Ok(Stored),
    /// Was renewed (and saved); use this one.
    Refreshed(Stored),
    /// No session, or the refresh token is dead too — a person has to approve a new code. The
    /// string says which stage said no, so the app log is enough to diagnose it.
    NeedsSignIn(&'static str),
}

/// Renew the OIDC tokens from the long-lived refresh token, then trade them for a fresh pair of DR
/// tokens. This is what keeps the set signed in for weeks after one manual approval.
fn renew(stored: &Stored) -> Result<Stored, &'static str> {
    let refresh = stored.oidc_refresh.as_str();
    let body = format!("grant_type=refresh_token&refresh_token={refresh}&client_id={CLIENT_ID}");
    let raw =
        form_post(&format!("{}/oidc/token", oidc_base()), body).ok_or("token endpoint unreachable")?;
    let reply: TokenReply = serde_json::from_str(&raw).map_err(|_| "token reply did not parse")?;
    if reply.access_token.is_empty() {
        // **Say WHICH refusal.** `invalid_grant` is the token itself being dead (single-use and
        // already spent, or past its own lifetime) and it is the only one that requires a person
        // to approve a new code; anything else is this request being wrong and is worth a bug. An
        // expired session used to log one line for both, which read like a code fault for weeks of
        // set logs.
        return Err(match reply.error.as_str() {
            "invalid_grant" => "refresh token expired or already used — sign in again",
            "invalid_client" => "the OIDC client was refused",
            "" => "token endpoint sent no access token and no error",
            _ => "token endpoint refused the refresh",
        });
    }
    // Some stacks rotate the refresh token and some hand back the same one; either way the value
    // to keep is the reply's when it sent one.
    let next = if reply.refresh_token.is_empty() {
        refresh.to_string()
    } else {
        reply.refresh_token
    };
    // PERSIST THE ROTATED TOKEN BEFORE THE EXCHANGE. DR's refresh tokens are single-use: if the
    // exchange below fails (a flaky network, DR briefly unhappy), the token we just used is spent,
    // and keeping only the old one on disk would leave the session permanently unrecoverable.
    if next != refresh {
        let mut carry = stored.clone();
        carry.oidc_refresh = next.clone();
        save(&carry);
    }
    let fresh = exchange(&reply.access_token, &reply.id_token, &next)
        .ok_or("exchange refused the refreshed tokens")?;
    // **The exchange always mints the DEFAULT profile's catalogue token.** A refresh twelve hours
    // into a session was therefore a silent move back to whoever the account calls default — and
    // every write this app makes names a profile (`api::set_watched`), so the cost is the wrong
    // person's viewing history. Re-seat the profile that was actually chosen, against the fresh
    // account token, through the same door the picker uses.
    let Some(seat) = wants_reseat(stored) else { return Ok(fresh) };
    let attempt = seat_from(fresh.clone(), &seat, &stored.segments);
    if let Err(why) = &attempt {
        crate::log(&format!(
            "dr-auth: a refresh could not re-seat profile {seat}: {} — on the account's default profile until the next picker",
            why.why()));
    }
    Ok(refreshed_record(fresh, &stored.auto_profile_id, attempt))
}

/// Which profile a refresh must re-seat: `None` when the session never chose one, because then the
/// default **is** the answer and re-seating would be a round trip to learn what the exchange
/// already said.
fn wants_reseat(previous: &Stored) -> Option<String> {
    (!previous.profile_id.is_empty()).then(|| previous.profile_id.clone())
}

/// What a refresh leaves behind. A successful re-seat is the record; a failed one is the
/// exchange's own — **and deliberately not the previous record with its name intact**, because a
/// `name` and a `profile` token must come out of the same exchange. The failure also leaves
/// `profile_id` at 0, so the next boot's roster read re-seats (or shows the picker) instead of a
/// stale id suppressing both.
fn refreshed_record(fresh: Stored, previous_pin: &str, seat: Result<Stored, SeatRefusal>) -> Stored {
    let mut record = match seat {
        Ok(seated) => seated,
        Err(_) => fresh,
    };
    // **The pin is the install's, not the token's.** `exchange` mints a record with none, so
    // without this a refresh every twelve hours would silently un-pin the household — the next
    // boot would come back to the picker with no explanation on screen. Carried on BOTH outcomes:
    // a refresh that failed to re-seat still has to remember what the set starts with, or the
    // viewer loses the pin to a flaky minute.
    record.auto_profile_id = previous_pin.to_string();
    record
}

/// Make sure a usable session exists, renewing it silently when it is close to expiry.
pub fn ensure_fresh() -> Freshness {
    let Some(stored) = load() else {
        return Freshness::NeedsSignIn("no session on disk");
    };
    if !needs_refresh(&stored, now()) {
        // A session saved before the name was carried has none, and the account menu would show a
        // placeholder until the next sign-in. One profile call fixes that for good.
        if stored.name.is_empty() {
            if let Some(name) = profile_name(&stored.profile) {
                let mut named = stored.clone();
                named.name = name;
                save(&named);
                return Freshness::Ok(named);
            }
        }
        return Freshness::Ok(stored);
    }
    if stored.oidc_refresh.is_empty() {
        return Freshness::NeedsSignIn("session has no refresh token");
    }
    match renew(&stored) {
        Ok(fresh) => {
            save(&fresh);
            Freshness::Refreshed(fresh)
        }
        Err(why) => Freshness::NeedsSignIn(why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The session file is a live DR identity on a shared partition. It must not be group- or
    /// world-readable, and it must not be created that way even for an instant.
    #[cfg(unix)]
    #[test]
    fn the_session_file_is_written_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("drtv-auth-mode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let fresh = dir.join("fresh.json");
        let _ = std::fs::remove_file(&fresh);
        write_owner_only(&fresh, "{\"profile\":\"p\"}").expect("write");
        assert_eq!(
            std::fs::metadata(&fresh).expect("stat").permissions().mode() & 0o777,
            0o600,
            "a fresh session file is owner-only"
        );

        // An older build created it 0644; the next save has to close that window.
        let legacy = dir.join("legacy.json");
        std::fs::write(&legacy, "{}").expect("pre-create");
        std::fs::set_permissions(&legacy, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        write_owner_only(&legacy, "{}").expect("rewrite");
        assert_eq!(
            std::fs::metadata(&legacy).expect("stat").permissions().mode() & 0o777,
            0o600,
            "a permissive file left by an older build is narrowed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_live_session_is_not_renewed() {
        let s = Stored {
            oidc_refresh: "r".into(),
            account: "a".into(),
            profile: "p".into(),
            session_id: "s".into(),
            user_id: "u".into(),
            expires_at: 10_000,
            profile_id: String::new(),
            name: "Loveless".into(),
            segments: Vec::new(),
            auto_profile_id: String::new(),
        };
        assert!(!needs_refresh(&s, 9_000), "well inside its life");
    }

    #[test]
    fn a_session_inside_the_margin_is_renewed() {
        let s = Stored {
            oidc_refresh: "r".into(),
            account: "a".into(),
            profile: "p".into(),
            session_id: "s".into(),
            user_id: "u".into(),
            expires_at: 10_000,
            profile_id: String::new(),
            name: "Loveless".into(),
            segments: Vec::new(),
            auto_profile_id: String::new(),
        };
        assert!(needs_refresh(&s, 9_800), "100s left is inside the 300s margin");
        assert!(needs_refresh(&s, 10_001), "already expired");
    }

    #[test]
    fn a_session_without_a_profile_is_not_usable() {
        let s = Stored {
            oidc_refresh: "r".into(),
            account: "a".into(),
            profile: String::new(),
            session_id: "s".into(),
            user_id: "u".into(),
            expires_at: 10_000,
            profile_id: String::new(),
            name: "Loveless".into(),
            segments: Vec::new(),
            auto_profile_id: String::new(),
        };
        assert!(needs_refresh(&s, 0));
    }

    #[test]
    fn instructions_read_as_the_screen_wants() {
        let c = DeviceCode {
            user_code: "TSDT-LNWR".into(),
            verification_uri: "https://www.dr.dk/aktiver".into(),
            ..Default::default()
        };
        let text = c.instructions();
        assert!(text.contains("www.dr.dk/aktiver"));
        assert!(text.contains("TSDT-LNWR"));
        assert!(!text.contains("https://"), "the scheme is noise on a television");
    }

    #[test]
    fn device_code_parses() {
        let raw = r#"{"user_code":"WBTV-JZQL","device_code":"mYlK","verification_uri":"https://www.dr.dk/aktiver","verification_uri_complete":"https://www.dr.dk/aktiver?code=WBTV-JZQL","expires_in":600,"interval":5}"#;
        let c: DeviceCode = serde_json::from_str(raw).unwrap();
        assert_eq!(c.user_code, "WBTV-JZQL");
        assert_eq!(c.interval, 5);
        assert_eq!(c.expires_in, 600);
    }

    #[test]
    fn token_reply_parses_both_arms() {
        let pending: TokenReply = serde_json::from_str(r#"{"error":"authorization_pending"}"#).unwrap();
        assert_eq!(pending.error, "authorization_pending");
        assert!(pending.access_token.is_empty());

        let ok: TokenReply =
            serde_json::from_str(r#"{"access_token":"a.b.c","id_token":"d.e.f","refresh_token":"r"}"#).unwrap();
        assert_eq!(ok.access_token, "a.b.c");
        assert_eq!(ok.refresh_token, "r");
    }

    /// The roster as DR actually answers `GET /account`: string ids, `name` (not `title`), no
    /// avatar, and the default named at the TOP level rather than flagged on a row.
    #[test]
    fn the_roster_is_the_accounts_profiles_with_string_ids() {
        // `r##` because a colour literal contains `"#`, which ends an `r#"` string.
        let raw = r##"{"id":"acct","email":"x@y","primaryProfileId":"6e5650ef","pinEnabled":true,
            "profiles":[
              {"id":"6e5650ef","name":"Loveless","isActive":true,"color":"#f00","pinEnabled":false,
               "segments":["drtv","optedout"]},
              {"id":"2f420095-056c-4edb-85da-ec25802d9368","name":"Test","isActive":true,
               "color":"#de4472","pinEnabled":true,"segments":["ultra","optedout"]}]}"##;
        let list = parse_profiles(raw).expect("a roster");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "6e5650ef");
        assert_eq!(list[0].name, "Loveless");
        assert!(list[0].is_default, "primaryProfileId names the default");
        assert!(!list[1].is_default, "and a row is not the default for being isActive");
        assert_eq!(list[1].id, "2f420095-056c-4edb-85da-ec25802d9368");
        assert_eq!(list[0].avatar, "", "this account has no avatars");
        assert_eq!(list[0].color, "#f00", "DR's own colour for the tile is carried, not invented");
        assert!(!list[0].pin, "pinEnabled is the row's own fact");
        assert!(list[1].pin, "and a PIN row says so");
        // `segments` is what the front page is composed for — see [`Universe`] and FORK.md Step 98.
        assert_eq!(list[0].segments, ["drtv", "optedout"]);
        assert_eq!(list[1].segments, ["ultra", "optedout"]);
    }

    #[test]
    fn a_roster_row_that_says_nothing_about_a_colour_or_a_pin_still_reads() {
        let list = parse_profiles(r#"{"profiles":[{"id":"a","name":"Bare"}]}"#).expect("a roster");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].color, "");
        assert!(!list[0].pin);
    }

    #[test]
    fn a_roster_row_without_an_id_is_skipped_and_an_unknown_envelope_is_not_an_empty_roster() {
        let list = parse_profiles(r#"{"profiles":[{"name":"No id"},{"id":"P","name":"Kept"}]}"#)
            .expect("a roster");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Kept");
        assert!(parse_profiles(r#"{"data":[]}"#).is_none(),
            "a shape this reader does not know is a failure to report, not a roster of none");
    }

    #[test]
    fn a_seat_written_by_the_old_shape_reads_as_nothing_chosen() {
        // The field was `i64` when this file guessed integer ids. A pre-fix file must still load —
        // and its number must not become a seat, because no DR route accepts one.
        let legacy = r#"{"oidc_refresh":"r","account":"a","profile":"p","session_id":"s",
            "user_id":"u","expires_at":1,"profile_id":1,"name":"Loveless"}"#;
        let loaded: Stored = serde_json::from_str(legacy).expect("a pre-fix record still loads");
        assert_eq!(loaded.profile_id, "");
        assert!(wants_reseat(&loaded).is_none(), "so a refresh does not re-seat a number");
    }

    /// **The universe is what a front page is composed for**, so this is the mapping the whole
    /// profile fix rests on: DR names it in the roster row, and the children's front page lives at
    /// `/page?path=/<word>` rather than at `/`. Verified against `production-cdn.dr-massive.com`:
    /// `segments=minisjang,optedout&path=/` answers a one-row "Gå til Minisjang" stub, and the same
    /// segments with `path=/minisjang` answer the real Minisjang page (FORK.md Step 98).
    #[test]
    fn the_universe_is_the_rows_own_word_and_its_own_path() {
        for word in ["minisjang", "ramasjang", "ultra"] {
            let u = Universe::of(&[word.to_string(), "optedout".to_string()]);
            assert_eq!(u.path, format!("/{word}"));
            assert_eq!(u.segments, format!("{word},optedout"));
        }
        let general = Universe::of(&["drtv".to_string(), "optedout".to_string()]);
        assert_eq!(general, Universe::general(), "`drtv` IS the general catalogue");
        assert_eq!(Universe::general().path, "/");
        assert_eq!(Universe::of(&[]), Universe::general(), "no row, no claim");
        assert_eq!(
            Universe::of(&["optedout".to_string()]),
            Universe::general(),
            "a consent flag is not a universe"
        );
        assert_eq!(
            Universe::of(&["nonsense".to_string()]),
            Universe::general(),
            "a word this does not know must not hide the catalogue in somebody's name"
        );
        assert_eq!(
            Universe::of(&[" Minisjang ".to_string()]).path,
            "/minisjang",
            "case and spacing are the client's to normalise, not DR's to repeat"
        );
    }

    /// A session file from before the universe was persisted still loads — and reads as the
    /// general catalogue, never as a children's one somebody never chose.
    #[test]
    fn a_session_written_before_the_universe_reads_as_the_general_catalogue() {
        let legacy = r#"{"oidc_refresh":"r","account":"a","profile":"p","session_id":"s",
            "user_id":"u","expires_at":1}"#;
        let loaded: Stored = serde_json::from_str(legacy).expect("a pre-98 record still loads");
        assert!(loaded.segments.is_empty());
        assert_eq!(loaded.universe(), Universe::general());
    }

    #[test]
    fn a_seated_universe_survives_the_session_file() {
        let mut r = record("P-1", "Test Kid (up to 3)");
        r.segments = vec!["minisjang".to_string(), "optedout".to_string()];
        let back: Stored = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back.universe().path, "/minisjang", "the seat remembers whose catalogue it is");
    }

    /// **The pin is the install's, and a refresh must not eat it.** Both outcomes matter: a
    /// re-seated record arrives with a name and a token from the new exchange and no pin, and a
    /// failed re-seat arrives as the exchange's own record — also with no pin. Neither may cost the
    /// household its start-up profile.
    #[test]
    fn a_refresh_keeps_the_start_up_pin() {
        let seated = record("P-2", "Test Kid (up to 3)");
        assert_eq!(refreshed_record(record("", ""), "P-2", Ok(seated)).auto_profile_id, "P-2");
        let failed = refreshed_record(record("", ""), "P-2", Err(SeatRefusal::Unreachable));
        assert_eq!(failed.auto_profile_id, "P-2", "a flaky minute is not a reason to un-pin");
        assert!(failed.profile_id.is_empty(), "and the seat is still gone, so the boot re-seats");
    }

    /// A record written before the pin existed is the normal case on every set in the field: it
    /// reads as UNPINNED, never as a guess at somebody.
    #[test]
    fn a_session_written_before_the_pin_reads_as_unpinned() {
        let legacy = r#"{"oidc_refresh":"r","account":"a","profile":"p","session_id":"s",
            "user_id":"u","expires_at":1}"#;
        let loaded: Stored = serde_json::from_str(legacy).expect("a pre-97 record still loads");
        assert!(loaded.auto_profile_id.is_empty());
    }

    /// The one door onto the file: `set_auto` writes, `auto_profile` reads back, and an empty id
    /// forgets. Graded through the file because that is where the pin actually lives — a struct
    /// round-trip would pass while the write path never ran.
    ///
    /// This is the ONLY test in the binary that steers `AUTH_PATH` (`OnceLock`, first call wins),
    /// and no other test reads or writes the session file, so the steering cannot race.
    #[test]
    fn the_pin_is_written_to_the_session_file_and_forgotten_by_an_empty_id() {
        let dir = std::env::temp_dir().join(format!("dr-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("auth.json");
        let _ = std::fs::remove_file(&path);
        set_auth_path(path.clone());
        assert!(save(&record("", "Loveless")), "the session lands on the steered path");
        assert_eq!(auto_profile(), None, "a fresh session is unpinned");

        assert!(set_auto("P-2"));
        assert_eq!(auto_profile().as_deref(), Some("P-2"));
        // Beside the seat, not instead of it: pinning answers the NEXT boot, not this one.
        assert_eq!(load().map(|s| s.profile_id), Some(String::new()));

        assert!(set_auto(""));
        assert_eq!(auto_profile(), None, "an empty id takes the pin back");
        let _ = std::fs::remove_file(&path);
    }

    fn record(profile_id: &str, name: &str) -> Stored {
        Stored { oidc_refresh: "r".into(), account: "a".into(), profile: "p".into(),
            session_id: "s".into(), user_id: "u".into(), expires_at: 10_000,
            profile_id: profile_id.into(), name: name.into(),
            segments: vec!["drtv".into(), "optedout".into()], auto_profile_id: String::new() }
    }

    #[test]
    fn a_seat_refuses_what_it_cannot_ask_for() {
        // Both of these used to be `None`, indistinguishable in a set log from DR's own refusal.
        assert!(matches!(seat_from(record("", ""), "", &[]), Err(SeatRefusal::NoProfileId)));
        let mut no_account = record("P-1", "Loveless");
        no_account.account.clear();
        assert!(matches!(seat_from(no_account, "P-1", &[]), Err(SeatRefusal::NoAccountToken)));
        assert_ne!(SeatRefusal::NoAccountToken.why(), SeatRefusal::NoProfileId.why());
    }

    #[test]
    fn a_refusal_names_the_status_it_got() {
        assert!(SeatRefusal::Refused(403).why().contains("403"), "the status IS the diagnosis");
        assert_ne!(SeatRefusal::Unreachable.why(), SeatRefusal::Refused(403).why());
        assert_ne!(SeatRefusal::NoProfileToken.why(), SeatRefusal::Refused(200).why());
    }

    #[test]
    fn a_refresh_reseats_only_a_profile_that_was_chosen() {
        assert_eq!(wants_reseat(&record("", "Default")), None, "the default IS the answer");
        assert_eq!(wants_reseat(&record("6e56", "Loveless")).as_deref(), Some("6e56"));
    }

    #[test]
    fn a_failed_reseat_never_claims_the_previous_name() {
        // The bug this locks: handing back the old record with its `name` intact while `profile`
        // holds the DEFAULT profile's token — a menu naming one person over another's credentials.
        let previous = record("6e56", "Loveless");
        let fresh = record("", "Default");
        let left = refreshed_record(fresh.clone(), &previous.auto_profile_id, Err(SeatRefusal::Refused(403)));
        assert_eq!(left.profile_id, "", "a stale seat id must not suppress the next picker");
        assert_eq!(left.name, "Default", "the name and the token come from one exchange");
        assert_ne!(left.name, previous.name);
        assert_eq!(left.profile, fresh.profile, "and so does the catalogue token");
        assert_eq!(refreshed_record(fresh, "", Ok(record("6e56", "Loveless"))).profile_id, "6e56");
    }
}
