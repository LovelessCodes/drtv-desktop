//! DR Massive wire types and the calls that produce them.
//!
//! Every shape here was verified against the live API (and is pinned as a fixture in the parent
//! repository). Unknown fields are ignored by serde, which is what we want: DR adds fields freely
//! and none of them have been load-bearing for the client.

use serde::Deserialize;

pub const API: &str = "https://prod95.dr-massive.com/api";

/// [`API`] unless `DRTV_API` names another server — see [`crate::base`].
pub fn api_base() -> String {
    crate::base(API, "DRTV_API")
}
/// The value of `device` the API expects from a webOS client; anything else returns
/// "Item was not found" rather than a stream list.
pub const DEVICE: &str = "tv_lg_webos";
const UA: &str = "Mozilla/5.0 (Web0S; Linux/SmartTV) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/68.0.3440.106 Safari/537.36";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Images {
    #[serde(default)]
    pub tile: String,
    #[serde(default)]
    pub wallpaper: String,
    #[serde(default)]
    pub poster: String,
    #[serde(default)]
    pub logo: String,
}

/// One entry of `/account/profile/watched` — the whole of DR's playback memory for an item.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Watch {
    /// Seconds watched, as DR stores it (not milliseconds).
    #[serde(default)]
    pub position: i64,
    #[serde(default, rename = "isFullyWatched")]
    pub fully: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Genre {
    #[serde(default)]
    pub title: String,
}

/// DR's `genres` is POLYMORPHIC: usually `[{"title":"Drama"}]`, but some lists send
/// `["ukendt"]`. Typing it as the object alone made the whole page fail to parse — and a failed
/// Home parse is not a cosmetic error here, it drops the app to the Plex sign-in screen.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum GenreRef {
    Name(String),
    Obj(Genre),
}

impl GenreRef {
    pub fn title(&self) -> &str {
        match self {
            GenreRef::Name(name) => name,
            GenreRef::Obj(genre) => &genre.title,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Item {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "shortDescription")]
    pub short_description: String,
    #[serde(default)]
    pub tagline: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub duration: i64, // seconds
    #[serde(default)]
    pub images: Images,
    #[serde(default)]
    pub custom_id: String,
    #[serde(default)]
    pub release_year: Option<i64>,
    #[serde(default, rename = "episodeNumber")]
    pub episode_number: i64,
    #[serde(default, rename = "episodeName")]
    pub episode_name: String,
    /// Season items only: how many episodes the season has. Commonly 0 on DR's own data, which is
    /// why `available_episode_count` is the fallback rather than an afterthought.
    #[serde(default, rename = "episodeCount")]
    pub episode_count: i64,
    #[serde(default, rename = "availableEpisodeCount")]
    pub available_episode_count: i64,
    #[serde(default, rename = "seasonId")]
    pub season_id: String,
    #[serde(default, rename = "showId")]
    pub show_id: String,
    #[serde(default, rename = "seasonNumber")]
    pub season_number: i64,
    /// Answered only when the read asked `expand=children`: a show's seasons and a season's
    /// episodes arrive as lists alongside the item itself.
    #[serde(default)]
    pub seasons: List,
    #[serde(default)]
    pub episodes: List,
    /// DR's editorial strings live here — `ClassificationRating` ("MRAM-11"),
    /// `PlayButtonExtraDetails` ("S5:E31 <name>"), `ContextualTitleExtraDetails`
    /// ("21. OKT 2026 | 28M"), `LinkTextBadge_Max_20_Characters`, `AccessServicesOffered`, …
    /// Typed as JSON because the values are strings, arrays and objects depending on the key.
    #[serde(default)]
    pub genres: Vec<GenreRef>,
    #[serde(default, rename = "customFields")]
    pub custom: std::collections::BTreeMap<String, serde_json::Value>,
}

impl Item {
    /// One of DR's editorial strings, if this item carries it.
    pub fn custom_str(&self, key: &str) -> Option<&str> {
        self.custom.get(key)?.as_str()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct List {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Entry {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub list: Option<List>,
    #[serde(default)]
    pub item: Option<Item>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Page {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Subtitle {
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub language: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TimeCode {
    #[serde(rename = "timeCodeType", default)]
    pub kind: String,
    /// **`startTime`/`endTime`, not `start_time`/`end_time`** — the same camelCase trap as
    /// [`File::is_stream_live`], and the more expensive one: without the rename every code parsed as
    /// `0..0`, and [`markers`] drops a code whose start is not after zero. So the skip prompt was
    /// dead twice over — the credits kind was spelled `EndCredit` and the seconds were never read.
    #[serde(default, rename = "startTime")]
    pub start_time: i64,
    #[serde(default, rename = "endTime")]
    pub end_time: i64,
}

/// One playable rendition. `drm` is `"None"` for everything DR produces itself and `"PlayReady"`
/// for some licensed films — the only titles a native client cannot play.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct File {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub drm: String,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub resolution: String,
    /// **`isStreamLive`, not `is_stream_live`** — the service spells its wire fields camelCase, so a
    /// snake_case field with no `rename` reads `false` for every file, live channels included.
    #[serde(default, rename = "isStreamLive")]
    pub is_stream_live: bool,
    /// Which access service this rendition is FOR. DR ships the same 1080p HLS twice —
    /// `StandardVideo` and the audio-described cut — with identical resolution and format, so
    /// picking by resolution alone hands a sighted viewer the described one. See [`File::described`]
    /// for the two markers that cut has carried.
    #[serde(default, rename = "accessService")]
    pub access_service: String,
    #[serde(default)]
    pub subtitles: Vec<Subtitle>,
    #[serde(rename = "timeCodes", default)]
    pub time_codes: Vec<TimeCode>,
}

impl File {
    /// A native client can play this itself only when nothing has to be decrypted.
    pub fn playable(&self) -> bool {
        self.drm.is_empty() || self.drm == "None"
    }

    /// Whether this rendition is the audio-**described** cut — a narrator over the dialogue — rather
    /// than the standard one.
    ///
    /// **Both markers the service has used.** It was `SpokenSubtitles` in the payloads this was
    /// first written against and it is `AudioDescription` in the one the live service answers today
    /// (measured 2026-10-28, and the pinned Borgen fixture agrees). A check that names one of them
    /// ranks the other as an ordinary second rendition, and since both are `HD-1080` the last one
    /// taken at that rung wins — which is how the described cut gets played.
    pub fn described(&self) -> bool {
        matches!(self.access_service.as_str(), "AudioDescription" | "SpokenSubtitles")
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScheduleSlot {
    #[serde(default)]
    pub start_date: String,
    #[serde(default)]
    pub item: Item,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChannelSchedule {
    #[serde(default)]
    pub channel_id: String,
    #[serde(default)]
    pub schedules: Vec<ScheduleSlot>,
}

// ------------------------------------------------------------------ transport

fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// One GET against the catalog API. Returns the body only on 2xx.
pub fn get(path: &str) -> Option<String> {
    let url = format!("{}{path}", api_base());
    let headers = vec![
        format!("User-Agent: {UA}"),
        "Origin: https://www.dr.dk".to_string(),
        "Referer: https://www.dr.dk/drtv/".to_string(),
        "Accept: application/json".to_string(),
    ];
    let resp = drtv_net::get(&url, &headers).ok()?;
    if !resp.ok() {
        return None;
    }
    String::from_utf8(resp.body).ok()
}

/// The installed DRTV app's own base and parameters, read out of its renderer's memory (its bundle
/// is NCG-encrypted, so this is the only place they are visible). `device=hbbtv` alone is NOT
/// enough: `distributor`, `ff` and the `segments`/`sub` pair are what make DR compose the television
/// front page instead of the three-link navigation stub that `/page?path=/` returns for every other
/// device string.
pub const TV_API: &str = "https://production-cdn.dr-massive.com/api";

/// [`TV_API`] unless `DRTV_TV_API` names another server.
pub fn tv_api_base() -> String {
    crate::base(TV_API, "DRTV_TV_API")
}
const TV_PARAMS: &str = "device=hbbtv&distributor=primary&ff=idp%2Cldp%2Crpt%2Csv2%2Cdpl%2Ccd&geoLocation=dk&isDeviceAbroad=false&isLive2VodSupported=false&lang=da";

/// [`TV_PARAMS`] plus the universe the seated profile belongs to. **`segments` is not a constant**:
/// with `segments=drtv` DR composes the adult front page for every profile, so a child's tile led
/// to the parent's catalogue (FORK.md Step 98). One function, so the page and the lists it names
/// can never be asked for different universes.
fn tv_params(universe: &crate::auth::Universe) -> String {
    format!("{TV_PARAMS}&segments={}", universe.segments.replace(',', "%2C"))
}

/// The television front page: nineteen entries in DR's own order (`Se videre`, `Mest sete lige
/// nu`, `Nye programmer`, `Nyheder`, `Filmfavoritter`, …). Items arrive separately via [`lists`].
pub fn home(universe: &crate::auth::Universe) -> Option<Page> {
    let url = format!(
        "{}/page?{}&list_page_size=24&max_list_prefetch=3&path={}&sub=Registered",
        tv_api_base(),
        tv_params(universe),
        percent_encode_path(&universe.path)
    );
    serde_json::from_str(&get_url(&url)?).ok()
}

/// One round trip for several lists at once, exactly as the app asks it: `ids=<id>|page_size=24,…`.
pub fn lists(ids: &[String], universe: &crate::auth::Universe) -> Vec<List> {
    if ids.is_empty() {
        return Vec::new();
    }
    let ids = ids.iter().map(|id| format!("{id}%7Cpage_size%3D24")).collect::<Vec<_>>().join("%2C");
    let url = format!("{}/lists?{}&ids={ids}", tv_api_base(), tv_params(universe));
    get_url(&url).and_then(|body| serde_json::from_str(&body).ok()).unwrap_or_default()
}

fn get_url(url: &str) -> Option<String> {
    let headers = vec![
        format!("User-Agent: {UA}"),
        "Origin: https://www.dr.dk".to_string(),
        "Referer: https://www.dr.dk/drtv/".to_string(),
        "Accept: application/json".to_string(),
    ];
    let resp = drtv_net::get(url, &headers).ok()?;
    if !resp.ok() {
        return None;
    }
    String::from_utf8(resp.body).ok()
}

pub fn page(path: &str) -> Option<Page> {
    let body = get(&format!("/page?path={}", percent_encode_path(path)))?;
    serde_json::from_str(&body).ok()
}

/// Everything the signed-in profile has watched, by item id. Empty (not an error) when signed out
/// or when DR will not answer: a missing progress bar must never keep Home from drawing.
pub fn watched(profile_token: &str) -> std::collections::BTreeMap<String, Watch> {
    if profile_token.is_empty() {
        return Default::default();
    }
    let headers = vec![
        format!("Authorization: Bearer {profile_token}"),
        format!("device: {DEVICE}"),
        "Accept: application/json".to_string(),
    ];
    let url = format!("{}/account/profile/watched?device={DEVICE}", api_base());
    let Ok(resp) = drtv_net::get(&url, &headers) else {
        return Default::default();
    };
    if !resp.ok() {
        return Default::default();
    }
    serde_json::from_slice(&resp.body).unwrap_or_default()
}

/// A `{…, items: [...]}` account list — the shape `/account/profile/*/list` answers with.
#[derive(Debug, Clone, Default, Deserialize)]
struct AccountList {
    #[serde(default)]
    items: Vec<Item>,
}

/// One of the profile's own lists. `path` is the URL half of the name DR uses in the front page's
/// entry ids (`continue-watching` for `ContinueWatching`) — see [`crate::card::user_list_path`].
pub fn user_list(profile_token: &str, path: &str) -> Vec<Item> {
    if profile_token.is_empty() {
        return Vec::new();
    }
    let headers = vec![
        format!("Authorization: Bearer {profile_token}"),
        format!("device: {DEVICE}"),
        "Accept: application/json".to_string(),
    ];
    let url = format!("{}/account/profile/{path}/list?device={DEVICE}", api_base());
    let Ok(resp) = drtv_net::get(&url, &headers) else {
        return Vec::new();
    };
    if !resp.ok() {
        return Vec::new();
    }
    serde_json::from_slice::<AccountList>(&resp.body).map(|list| list.items).unwrap_or_default()
}

/// "Se videre" — what the profile has started and not finished, most recent first.
pub fn continue_watching(profile_token: &str) -> Vec<Item> {
    user_list(profile_token, "continue-watching")
}

/// "Min liste" — the profile's bookmarks.
pub fn bookmarks(profile_token: &str) -> Vec<Item> {
    user_list(profile_token, "bookmarks")
}

/// How many episodes a season has, from the season's own item. `0` when DR will not say.
/// How many episodes a season has.
///
/// `episodeCount` is DR's own total but is 0 on many seasons (Vigil S3 reports 0 and 6 available);
/// `availableEpisodeCount` is the number the client is shown. Prefer the total when DR states one.
pub fn episode_count(season_id: &str) -> i64 {
    match item(season_id) {
        Some(season) if season.episode_count > 0 => season.episode_count,
        Some(season) => season.available_episode_count,
        None => 0,
    }
}

/// An item WITH its children: `?expand=children` (the enum is `all|children|ancestors|parent`,
/// verified against the API). A show answers `seasons`, a season answers `episodes` — the call the
/// installed DRTV app makes to fill its season bar and episode shelf.
pub fn expanded(id: &str) -> Option<Item> {
    let body = get(&format!("/items/{id}?device={DEVICE}&expand=children&item_limit=100"))?;
    serde_json::from_str(&body).ok()
}

pub fn item(id: &str) -> Option<Item> {
    let body = get(&format!("/items/{id}"))?;
    serde_json::from_str(&body).ok()
}

/// Which rendition to ask DR for.
///
/// `Best` names the service's top rung (`HD-4K`); DR answers with that file where a title has one
/// and falls back to the 1080p file otherwise. `Hd1080` pins the HD rendition. The API's own enum
/// is `HD-4K | HD-1080 | HD-720 | SD | External`; this client never asks below HD because the
/// catalogue has nothing there (probed 2026-10-25: `HD-720` and `SD` answer
/// `404 {"code":8009,"message":"Ingen filer"}`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Best,
    Hd1080,
}

impl Resolution {
    fn query(self) -> &'static str {
        match self {
            Resolution::Best => "HD-4K",
            Resolution::Hd1080 => "HD-1080",
        }
    }
}

pub fn videos(id: &str, resolution: Resolution) -> Option<Vec<File>> {
    let body = get(&format!(
        "/items/{id}/videos?delivery=stream&resolution={}&formats=hls&device={DEVICE}",
        resolution.query()
    ))?;
    serde_json::from_str(&body).ok()
}

/// The rendition a native client should play: **standard audio**, not the audio-described cut, not
/// live, not DRM'd, best resolution first.
///
/// DR lists both 1080p renditions with identical `resolution`/`format`, so `accessService` is the
/// only field that tells them apart — picking by resolution alone played the described-audio cut,
/// whose manifest then failed on the transport (`rc=28`). That makes the described cut a
/// **preference, not a filter**: a title that ships only that one still plays, and a filter would
/// have dropped it on the floor.
///
/// The service's UHD rung is spelled `HD-4K` (the API rejects `UHD-2160` as an enum value), so
/// that is the one the rank names above `HD-1080`.
pub fn stream_file(files: &[File]) -> Option<&File> {
    let rank = |f: &File| match f.resolution.as_str() {
        "HD-4K" => 4,
        "HD-1080" => 3,
        "HD" => 2,
        "SD" => 1,
        _ => 0,
    };
    files
        .iter()
        .filter(|f| f.playable() && !f.is_stream_live && !f.url.is_empty())
        // Ties are broken by SERVICE, and only after the rung: the higher resolution is still the
        // better file, and at the same rung the standard cut is the one a sighted viewer asked for.
        .max_by_key(|f| (rank(f), !f.described()))
}

/// Write playback progress for one item: `position` SECONDS, which is the whole of DR's watch
/// state — `isFullyWatched` is what the service decides from it, not a flag the client sends.
///
/// Verified against the live service (`../drtv-native/NOTES.md`, "The progress write, found and
/// verified"), and three parts of that are not optional:
///
/// - **PUT**, and only PUT, for PROGRESS. Un-watching is not a write at all — it is a delete from
///   the collection ([`clear_watched`]); a position-0 PUT leaves `isFullyWatched` standing.
/// - **A body is REQUIRED** — `{}`. Without one the CDN answers an HTML `400`, which reads like a
///   dead endpoint rather than a missing body.
/// - `position` past the item's duration is what makes the service call it fully watched; the
///   probe used 60000 s for that. [`Self::watched_position`] picks the honest number per item.
/// - `Err(status)` carries the HTTP status, or `-1` for no response at all — the caller owns the log
///   line, because this crate may not name the app's event log (module layering).
pub fn set_watched(profile_token: &str, item_id: &str, position_secs: i64) -> Result<(), i32> {
    if profile_token.is_empty() || item_id.is_empty() {
        return Err(-1);
    }
    let headers = vec![
        format!("Authorization: Bearer {profile_token}"),
        format!("device: {DEVICE}"),
        "Accept: application/json".to_string(),
        "Content-Type: application/json".to_string(),
    ];
    let url = format!(
        "{}/account/profile/watched/{}?device={DEVICE}&position={}",
        api_base(),
        percent_encode_path(item_id),
        position_secs.max(0)
    );
    match drtv_net::put(&url, &headers, b"{}") {
        Ok(resp) if resp.ok() => Ok(()),
        Ok(resp) => Err(i32::from(resp.status)),
        Err(_) => Err(-1),
    }
}

/// **Un-watch**: delete the item's record from the watched collection.
///
/// The record is removed from the COLLECTION, filtered by `?item_ids=` — `DELETE
/// /account/profile/watched/{itemId}` answers `405`, and a `PUT` at position 0 does not un-watch:
/// it writes position 0 into a record whose `isFullyWatched` stays `true` (measured on the live
/// service, 2026-10-23, on an item that had been marked watched). DR's own web client does exactly
/// this: its api map carries `{path:"/account/profile/watched", method:"delete"}` and the action
/// sends `item_ids` as a CSV.
pub fn clear_watched(profile_token: &str, item_ids: &[&str]) -> Result<(), i32> {
    let ids: Vec<&str> = item_ids.iter().copied().filter(|id| !id.is_empty()).collect();
    if profile_token.is_empty() || ids.is_empty() {
        return Err(-1);
    }
    let headers = vec![
        format!("Authorization: Bearer {profile_token}"),
        format!("device: {DEVICE}"),
        "Accept: application/json".to_string(),
    ];
    let url = format!(
        "{}/account/profile/watched?device={DEVICE}&item_ids={}",
        api_base(),
        ids.join("%2C")
    );
    match drtv_net::delete(&url, &headers) {
        Ok(resp) if resp.ok() => Ok(()),
        Ok(resp) => Err(i32::from(resp.status)),
        Err(_) => Err(-1),
    }
}

/// DR's `timeCodes` as skip segments. The service's own codes are `Summary`, `Intro` and
/// `EndCredit`; the skip prompt knows the intro and the credits, so those two are what this reads.
/// `Credits` is accepted as well — it is what the payloads this was first written against said, and
/// a code is a word from the service, not from us. Times are in SECONDS, and a segment with no end
/// or no start is not a segment.
pub fn markers(file: &File) -> Vec<(bool, i64, i64)> {
    file.time_codes
        .iter()
        .filter_map(|c| {
            let credits =
                c.kind.eq_ignore_ascii_case("EndCredit") || c.kind.eq_ignore_ascii_case("Credits");
            let intro = c.kind.eq_ignore_ascii_case("Intro");
            if !(credits || intro) || c.start_time <= 0 || c.end_time <= c.start_time {
                return None;
            }
            Some((credits, c.start_time.saturating_mul(1000), c.end_time.saturating_mul(1000)))
        })
        .collect()
}

/// DR's subtitle marker as a label a menu can show: `ForeignLanguageSubtitles` → `Foreign`,
/// `DanishLanguageSubtitles` → `Danish`.
///
/// The service's own vocabulary, trimmed — not translated. A name this build does not recognise is
/// passed through as it stands rather than dropped: an oddly named track is still a track.
pub fn subtitle_label(marker: &str) -> String {
    // The two tracks DR publishes per item, named by what they CONTAIN rather than by the API's
    // marker: "Combined" is the complete Danish track (foreign dialogue plus Danish, with sound
    // descriptions — the file name says `Foreign_HardOfHearing`), "Foreign" is the foreign-dialogue
    // lines only. `Combined`/`Foreign` alone read as jargon in a track menu.
    match marker {
        "CombinedLanguageSubtitles" => return "Dansk".to_string(),
        "ForeignLanguageSubtitles" => return "Foreign dialogue".to_string(),
        _ => {}
    }
    let trimmed = marker
        .strip_suffix("LanguageSubtitles")
        .or_else(|| marker.strip_suffix("Subtitles"))
        .unwrap_or(marker)
        .trim();
    if trimmed.is_empty() { "Subtitles".to_string() } else { trimmed.to_string() }
}

/// The `position` to WRITE while playing, from the player's own numbers.
///
/// Two rules, both about the service rather than the client: it stores whole seconds (the endpoint
/// takes `position=<seconds>`), and it decides `isFullyWatched` itself — so a play that ends within
/// a whisker of the end must send the END, or a film watched to the credits sits in Continue
/// Watching at "59:58 of 1:00:00" for ever. Everything else is the honest position, rounded down:
/// rounding up would claim a second the viewer has not seen.
pub fn progress_position(time_ms: i64, duration_ms: i64) -> i64 {
    if time_ms <= 0 {
        return 0;
    }
    let t = time_ms / 1000;
    let d = duration_ms / 1000;
    if d > 0 && t * 100 >= d * 95 {
        return d;
    }
    t
}

/// The `position` a [`set_watched`] call should carry: the item's own length, so the service's own
/// `isFullyWatched` follows, and a number past any episode when DR will not state one.
///
/// There is no "0" arm: un-watching is not a position at all ([`clear_watched`]), which is the
/// distinction this function's old `on` flag got wrong.
pub fn watched_position(item: &Item) -> i64 {
    if item.duration > 0 { item.duration } else { 60_000 }
}

pub fn schedules(
    channel: &str,
    date: &str,
    hour: u32,
    duration: u32,
) -> Option<Vec<ChannelSchedule>> {
    let body =
        get(&format!("/schedules?channels={channel}&date={date}&hour={hour}&duration={duration}"))?;
    serde_json::from_str(&body).ok()
}

/// One result group of a [`SearchReply`]. With `group=true` the endpoint answers in these
/// sections instead of one flat list, and each carries its own paging.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchGroup {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub total: i64,
}

/// `GET /v2/search` — the shape DR's own web client consumes. The sections that can carry
/// programmes are kept; `people` (an array of a different shape), `events`, `teams`,
/// `competitions`, `confederations` and `newshighlights` are not, because a DRTV fork has nothing
/// to draw them into.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchReply {
    #[serde(default)]
    pub term: String,
    #[serde(default)]
    pub total: i64,
    /// Movies AND episodes, mixed — it is what DR's own search page lists first.
    #[serde(default)]
    pub playable: SearchGroup,
    #[serde(default)]
    pub series: SearchGroup,
    #[serde(default)]
    pub movies: SearchGroup,
    #[serde(default)]
    pub tv: SearchGroup,
}

/// Search the catalog.
///
/// **Verified against the live service** (2026-10-22), and every part of that verification is a
/// thing that looks optional and is not:
///
/// - The path is `/v2/search`, ONE VERSION AHEAD of every other read in this module, and it answers
///   `405` to `/search`, `/v1/search` and `/search/v2`.
/// - The term parameter is **`term`**, not `q`: `/search?q=` says `query requires property "term"`.
/// - `sessionId` is REQUIRED and is **client-generated** — DR's web client makes a v4 UUID on first
///   visit and reuses it. It is not a server-issued session, so a fresh id works; see [`session_id`].
/// - `sub=Anonymous2` is how the endpoint is told the caller is not signed in. With a profile token
///   it is omitted; without one it is what makes the same call answer at all.
/// - `group=true` is what produces the named sections ([`SearchReply::playable`] and friends)
///   rather than one flat list.
/// - **`segments` is the seated profile's universe**, the same parameter the front page is asked
///   for with ([`tv_params`]). It was hard-coded to `drtv,optedout` here long after the front page
///   stopped being, which meant a child's profile searched the ADULT catalogue: the page it landed
///   on was `minisjang`'s and the results were everybody's. A signed-out search names the general
///   catalogue, because that is the only one an anonymous client may ask for.
///
/// A signed-out search answers in full: the endpoint treats the term as public catalogue, so the
/// desktop simulator can search with no DR session at all.
pub fn search(
    term: &str,
    profile_token: &str,
    universe: &crate::auth::Universe,
) -> Option<SearchReply> {
    let mut headers = vec![
        format!("User-Agent: {UA}"),
        "Origin: https://www.dr.dk".to_string(),
        "Referer: https://www.dr.dk/drtv/".to_string(),
        "Accept: application/json".to_string(),
        format!("device: {DEVICE}"),
    ];
    if !profile_token.is_empty() {
        headers.push(format!("Authorization: Bearer {profile_token}"));
    }
    let resp =
        drtv_net::get(&search_url(term, profile_token.is_empty(), universe), &headers).ok()?;
    if !resp.ok() {
        return None;
    }
    serde_json::from_str(&String::from_utf8(resp.body).ok()?).ok()
}

/// The request line [`search`] dials: a function of the term, whether there is a token, and the
/// seated universe — and of nothing else, so the universe the query names is gradeable without a
/// network call. `anonymous` appends DR's `sub=Anonymous2`, which is how the endpoint is told the
/// caller is not signed in; it is the caller's token that decides, not this function's guess.
fn search_url(term: &str, anonymous: bool, universe: &crate::auth::Universe) -> String {
    let sub = if anonymous { "&sub=Anonymous2" } else { "" };
    format!(
        "{}/v2/search?device={DEVICE}&ff=idp%2Cldp%2Crpt&group=true&lang=da\
         &segments={}&sessionId={}&term={}{sub}",
        tv_api_base(),
        universe.segments.replace(',', "%2C"),
        session_id(),
        percent_encode_path(term),
    )
}

/// The client-generated session id [`search`] requires, made once per process.
///
/// **A UUID SHAPE, not just any string.** The endpoint accepted every `/v2/search` probe that had
/// *some* `sessionId` EXCEPT the television's own stored `session_id` (a playback session, 36 hex
/// characters with no dashes), which came back `400 Bad Request` — so the value is parsed rather
/// than merely echoed, and a v4 UUID is the shape the client that owns this endpoint sends.
fn session_id() -> &'static str {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(|| {
        let mut x = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
            ^ ((std::process::id() as u64) << 32)
            | 0x9e37_79b9_7f4a_7c15;
        let mut bytes = [0u8; 16];
        for b in bytes.iter_mut() {
            // xorshift64: this is an identifier, not a secret — it only has to differ between two
            // processes started inside the same nanosecond, which the pid fold above also helps.
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = (x >> 24) as u8;
        }
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
    })
}

#[cfg(test)]
mod tests {
    /// **`segments` is not a constant.** It was `drtv,optedout` for every profile, which is why a
    /// child's tile on the dev set led to the adult front page; the parameter decides which
    /// catalogue DR composes, and the page's path decides whose front page it is.
    #[test]
    fn the_front_page_is_asked_for_in_the_seated_universe() {
        let kid = crate::auth::Universe::of(&["minisjang".to_string(), "optedout".to_string()]);
        let q = tv_params(&kid);
        assert!(q.contains("&segments=minisjang%2Coptedout"), "{q}");
        assert!(!q.contains("drtv"), "the adult catalogue is not the default any more: {q}");
        assert_eq!(percent_encode_path(&kid.path), "%2Fminisjang");
        let adult = tv_params(&crate::auth::Universe::general());
        assert!(adult.contains("&segments=drtv%2Coptedout"), "{adult}");
    }

    /// **Search asks in the SEATED universe too.** The front page's own test above is what caught
    /// the adult front page on a child's tile; this is the same bug one endpoint later — search kept
    /// `segments=drtv%2Coptedout` after the front page stopped, so a Minisjang profile searched the
    /// whole catalogue from a page that was otherwise the child's.
    #[test]
    fn a_search_is_asked_for_in_the_seated_universe() {
        let kid = crate::auth::Universe::of(&["minisjang".to_string(), "optedout".to_string()]);
        let ours = search_url("bluey", true, &kid);
        assert!(ours.contains("&segments=minisjang%2Coptedout"), "{ours}");
        assert!(
            !ours.contains("drtv"),
            "the adult catalogue is not search's default any more: {ours}"
        );
        assert!(ours.contains("&sub=Anonymous2"), "a signed-out search says so: {ours}");
        let adult = search_url("bluey", false, &crate::auth::Universe::general());
        assert!(adult.contains("&segments=drtv%2Coptedout"), "{adult}");
        assert!(!adult.contains("sub="), "a token is not anonymity: {adult}");
    }

    use super::*;

    // Shapes taken from the pinned fixtures; if DR renames a field these fail before the TV does.
    const PAGE: &str = r#"{"title":"Film","path":"/kategorier/film","entries":[
        {"type":"ListEntry","title":"Netop tilføjet","list":{"id":"1","items":[
            {"id":"628786","type":"program","title":"Den usynlige kvinde","duration":6144,
             "images":{"tile":"https://cdn/t.jpg","wallpaper":"https://cdn/w.jpg"}}]}}]}"#;

    const FILES: &str = r#"[{"url":"https://drod20i.akamaized.net/all/clear/none/x/master_manifest.m3u8",
        "drm":"None","format":"video/hls","resolution":"HD-1080","isStreamLive":false,
        "subtitles":[{"link":"https://cdn/sub.vtt","language":"DanishLanguageSubtitles"}],
        "timeCodes":[{"timeCodeType":"Intro","startTime":280,"endTime":299}]}]"#;

    #[test]
    fn page_parses_shelves() {
        let p: Page = serde_json::from_str(PAGE).unwrap();
        assert_eq!(p.entries.len(), 1);
        let list = p.entries[0].list.as_ref().unwrap();
        assert_eq!(list.items[0].id, "628786");
        assert_eq!(list.items[0].kind, "program");
        assert_eq!(list.items[0].duration, 6144);
        assert!(list.items[0].images.tile.ends_with(".jpg"));
    }

    #[test]
    fn files_parse_and_are_playable() {
        let f: Vec<File> = serde_json::from_str(FILES).unwrap();
        assert!(f[0].playable());
        assert_eq!(f[0].subtitles[0].language, "DanishLanguageSubtitles");
        assert_eq!(f[0].time_codes[0].kind, "Intro");
    }

    #[test]
    fn playready_is_not_playable() {
        let f = File { drm: "PlayReady".into(), ..Default::default() };
        assert!(!f.playable());
    }

    #[test]
    fn path_encoding() {
        assert_eq!(percent_encode_path("/kategorier/film"), "%2Fkategorier%2Ffilm");
        assert_eq!(percent_encode_path("/"), "%2F");
    }

    /// `GET /v2/search?group=true` with the keys the live service answers (2026-10-22), values
    /// replaced. The two groups that carry programmes are the two this client reads; a rename in
    /// either fails here, before a television shows an empty shelf with no explanation.
    const SEARCH: &str = r#"{"term":"x","total":2,
        "playable":{"id":"playable","title":"Programmer","total":1,"path":"/search",
          "items":[{"id":"1","type":"episode","title":"E1","seasonId":"7","showId":"2",
                    "episodeNumber":3,"duration":1200}]},
        "series":{"id":"series","title":"Serier","total":1,"path":"/search",
          "items":[{"id":"2","type":"show","title":"S","availableSeasonCount":3}]},
        "movies":{"id":"movies","title":"Film","total":0,"items":[]},
        "people":[], "events":{"id":"events","items":[]}}"#;

    /// The two buttons are one endpoint with two numbers: the item's own length for "watched" (so
    /// the service's `isFullyWatched` follows), zero for "not watched", and a number past any
    /// episode when DR will not state a length.
    /// The two halves of the button write to DIFFERENT shapes, and the item-scoped DELETE that looks
    /// obvious is the one the service refuses (`405`).
    /// The end of a play is written as the END, not as the last tick: the service's own
    /// `isFullyWatched` follows the position, so a film watched to the credits must not sit at
    /// 59:58 for ever. Below that, the position is rounded DOWN — a claimed second nobody saw is
    /// worse than an unclaimed one they did.
    #[test]
    fn the_written_position_is_the_honest_second_and_the_end_is_the_end() {
        assert_eq!(progress_position(0, 1_710_000), 0);
        assert_eq!(progress_position(61_900, 1_710_000), 61);
        assert_eq!(progress_position(1_700_000, 1_710_000), 1_710, "past 95% writes the end");
        assert_eq!(progress_position(1_710_000, 1_710_000), 1_710);
        assert_eq!(progress_position(500_000, 0), 500, "no duration: the position as it stands");
    }

    /// Seconds in, milliseconds out, and only the two kinds the skip prompt knows. A code with no
    /// end (or a zero start) is dropped rather than turned into a zero-length prompt.
    ///
    /// **Both spellings of the credits code.** The live payloads this test was written against said
    /// `Credits`; the pinned Borgen fixture and today's `/videos` say `EndCredit`, and a reader that
    /// names one of them silently has no credits marker for the other — which is a prompt that never
    /// appears, not an error anybody would see.
    #[test]
    fn time_codes_become_skip_markers() {
        let file = File {
            time_codes: vec![
                TimeCode { kind: "Intro".into(), start_time: 280, end_time: 299 },
                TimeCode { kind: "Credits".into(), start_time: 1600, end_time: 1710 },
                TimeCode { kind: "EndCredit".into(), start_time: 3361, end_time: 3407 },
                TimeCode { kind: "Intro".into(), start_time: 0, end_time: 30 },
                TimeCode { kind: "Recap".into(), start_time: 5, end_time: 9 },
                TimeCode { kind: "Intro".into(), start_time: 40, end_time: 40 },
            ],
            ..Default::default()
        };
        assert_eq!(
            markers(&file),
            vec![
                (false, 280_000, 299_000),
                (true, 1_600_000, 1_710_000),
                (true, 3_361_000, 3_407_000),
            ]
        );
        assert!(markers(&File::default()).is_empty());
    }

    #[test]
    fn a_subtitle_marker_becomes_a_label_and_never_an_empty_one() {
        assert_eq!(subtitle_label("ForeignLanguageSubtitles"), "Foreign dialogue");
        assert_eq!(subtitle_label("CombinedLanguageSubtitles"), "Dansk", "the complete track");
        assert_eq!(subtitle_label("DanishLanguageSubtitles"), "Danish");
        assert_eq!(subtitle_label("DanishHardOfHearingLanguageSubtitles"), "DanishHardOfHearing");
        assert_eq!(subtitle_label("Karaoke"), "Karaoke", "an unknown name is still a name");
        assert_eq!(subtitle_label(""), "Subtitles");
        assert_eq!(subtitle_label("Subtitles"), "Subtitles");
    }

    /// The two 1080p renditions differ only by `accessService`; the standard one wins even when it
    /// is listed FIRST (a `max_by_key` that ties takes the last, which is how the described-audio
    /// cut got played).
    ///
    /// **Both spellings the service has used for that cut.** It was `SpokenSubtitles` when this
    /// test was written and it is `AudioDescription` in the payload the live service answers today,
    /// so a check that names one of them stops discriminating the moment the service renames the
    /// other — which is exactly what happened, silently, because a differently-spelled second
    /// rendition is just another candidate at the same resolution.
    #[test]
    fn the_standard_rendition_beats_the_described_one_at_equal_resolution() {
        let standard = File {
            url: "s.m3u8".into(),
            resolution: "HD-1080".into(),
            access_service: "StandardVideo".into(),
            ..Default::default()
        };
        for marker in ["SpokenSubtitles", "AudioDescription"] {
            let described = File {
                url: "d.m3u8".into(),
                resolution: "HD-1080".into(),
                access_service: marker.into(),
                ..Default::default()
            };
            for files in [
                vec![described.clone(), standard.clone()],
                vec![standard.clone(), described.clone()],
            ] {
                assert_eq!(
                    stream_file(&files).map(|f| f.url.as_str()),
                    Some("s.m3u8"),
                    "the {marker} cut is not the file a sighted viewer asked for"
                );
            }
            // …but the described cut is still a stream: a title that ships only that one must play
            // rather than fail, so the preference is a tiebreak and never a filter.
            assert!(stream_file(&[described]).is_some(), "{marker} alone is still playable");
        }
        let drm = File {
            url: "x.m3u8".into(),
            resolution: "HD-4K".into(),
            drm: "PlayReady".into(),
            ..Default::default()
        };
        assert_eq!(stream_file(&[drm]).map(|f| f.url.as_str()), None, "a DRM'd file cannot play");
        assert!(stream_file(&[]).is_none());
    }

    /// `isStreamLive` is how the service spells it, and the field had no `rename`, so it was never
    /// read: every file looked on demand and a live channel would have been handed to the VOD
    /// pipeline. The file below is the live payload's own spelling, key for key.
    #[test]
    fn the_services_time_codes_are_read_in_its_own_spelling() {
        // The pinned Borgen episode's `/videos` payload, verbatim. `startTime`/`endTime` are the
        // service's spelling; a struct field without a rename reads 0, and a zero start is what
        // `markers` refuses — so this asserts the seconds, not just the kinds.
        let payload = r#"[{"url":"https://cdn/v.mp4","drm":"None","format":"video/hls",
            "resolution":"HD-1080","accessService":"StandardVideo","isStreamLive":false,
            "timeCodes":[{"timeCodeType":"Summary","startTime":0,"endTime":56},
                         {"timeCodeType":"Intro","startTime":273,"endTime":327},
                         {"timeCodeType":"EndCredit","startTime":3361,"endTime":3407}]}]"#;
        let files: Vec<File> = serde_json::from_str(payload).unwrap();
        assert_eq!(files[0].time_codes[1].start_time, 273, "the service spells it startTime");
        assert_eq!(
            markers(&files[0]),
            vec![(false, 273_000, 327_000), (true, 3_361_000, 3_407_000)],
            "the intro and the credits survive the service's own spelling"
        );
    }

    #[test]
    fn a_live_rendition_is_read_from_the_services_own_spelling() {
        let live = r#"[{"url":"https://cdn/live.m3u8","drm":"None","format":"video/hls",
            "resolution":"HD-1080","accessService":"StandardVideo","isStreamLive":true,
            "timeCodes":[]}]"#;
        let files: Vec<File> = serde_json::from_str(live).unwrap();
        assert!(files[0].is_stream_live, "the service spells it isStreamLive");
        assert!(stream_file(&files).is_none(), "a live stream is not a file to play on demand");
    }

    /// The best rung the service names is `HD-4K`, not `UHD-2160` (which is not even in its enum),
    /// and it must beat the 1080p file without depending on list order.
    #[test]
    fn the_uhd_rendition_beats_the_hd_one() {
        let uhd = File {
            url: "u.m3u8".into(),
            resolution: "HD-4K".into(),
            access_service: "StandardVideo".into(),
            ..Default::default()
        };
        let hd = File {
            url: "h.m3u8".into(),
            resolution: "HD-1080".into(),
            access_service: "StandardVideo".into(),
            ..Default::default()
        };
        for files in [vec![uhd.clone(), hd.clone()], vec![hd, uhd]] {
            assert_eq!(stream_file(&files).map(|f| f.url.as_str()), Some("u.m3u8"));
        }
    }

    #[test]
    fn the_quality_choices_name_the_services_own_resolutions() {
        assert_eq!(Resolution::Best.query(), "HD-4K");
        assert_eq!(Resolution::Hd1080.query(), "HD-1080");
    }

    #[test]
    fn un_watching_deletes_from_the_collection_with_item_ids() {
        let url = format!(
            "{API}/account/profile/watched?device={DEVICE}&item_ids={}",
            ["622937"].join("%2C")
        );
        assert!(
            url.ends_with("/account/profile/watched?device=tv_lg_webos&item_ids=622937"),
            "{url}"
        );
        assert!(!url.contains("/watched/622937"), "the ITEM-scoped delete is the 405");
    }

    #[test]
    fn a_watched_write_uses_the_items_own_length() {
        let episode = Item { duration: 3192, ..Default::default() };
        assert_eq!(watched_position(&episode), 3192);
        let silent = Item { duration: 0, ..Default::default() };
        assert_eq!(watched_position(&silent), 60_000, "past any episode DR will not measure");
    }

    #[test]
    fn search_reply_parses_its_groups() {
        let r: SearchReply = serde_json::from_str(SEARCH).unwrap();
        assert_eq!(r.total, 2);
        assert_eq!(r.playable.title, "Programmer");
        assert_eq!(r.playable.items[0].kind, "episode");
        assert_eq!(r.playable.items[0].season_id, "7");
        assert_eq!(r.playable.items[0].show_id, "2");
        assert_eq!(r.series.items[0].kind, "show");
        assert!(r.movies.items.is_empty());
        // Absent groups deserialize empty rather than failing: a service that stops sending one
        // must cost that shelf, not the whole answer.
        assert!(r.tv.items.is_empty());
    }

    /// The session id DR's endpoint requires is *parsed*, not echoed (the television's own stored
    /// `session_id` was refused with `400 Bad Request`), so its shape is part of the contract.
    #[test]
    fn the_search_session_id_is_a_stable_v4_uuid() {
        let a = session_id();
        assert_eq!(a, session_id(), "one id per process");
        let fields: Vec<usize> = a.split('-').map(str::len).collect();
        assert_eq!(fields, vec![8, 4, 4, 4, 12], "uuid v4 field lengths: {a}");
        assert_eq!(&a[14..15], "4", "version nibble: {a}");
        assert!(matches!(&a[19..20], "8" | "9" | "a" | "b"), "variant nibble: {a}");
        assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit()), "{a}");
    }
}
