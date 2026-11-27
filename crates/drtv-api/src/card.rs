//! DR's wire items → the cards the UI draws.
//!
//! This is the one place a DR field name is read for presentation. Screens ask a [`Card`] for its
//! title, its picture and its progress, and never learn that DR spells a film `program` or hides the
//! episode number in a play-button label.

use std::collections::BTreeMap;

use crate::api::{self, Item, Watch};

/// What a card *is*, in the app's vocabulary rather than DR's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// DR's `program` — a feature film or a one-off.
    Movie,
    Show,
    Season,
    Episode,
    Other,
}

impl Kind {
    fn parse(kind: &str) -> Self {
        match kind {
            // **DR spells a feature `program`** — it never sends `movie`.
            "program" => Kind::Movie,
            "show" => Kind::Show,
            "season" => Kind::Season,
            "episode" => Kind::Episode,
            _ => Kind::Other,
        }
    }

    /// Whether opening this card means playing it, rather than opening a page about it.
    pub fn playable(self) -> bool {
        matches!(self, Kind::Movie | Kind::Episode)
    }

    /// Whether the page needs a season bar and an episode list.
    pub fn container(self) -> bool {
        matches!(self, Kind::Show | Kind::Season)
    }
}

/// One tile.
#[derive(Debug, Clone, Default)]
pub struct Card {
    pub id: String,
    pub kind: String,
    pub title: String,
    /// The episode's own name (`episodeName`) when it has one; empty for films and shows.
    pub episode_name: String,
    pub description: String,
    /// 2:3 art for the portrait tile. For an episode this is the SHOW's key art, because DR fills
    /// an episode's `poster` with it.
    pub poster: String,
    /// 16:9 art the item owns: an episode's own still, a show's wallpaper. Landscape tiles and the
    /// detail backdrop use this, and it must never be the show's poster.
    pub still: String,
    pub year: Option<i64>,
    /// Seconds. Zero when DR does not say.
    pub duration_s: i64,
    /// Milliseconds already watched, or 0. DR stores seconds.
    pub resume_ms: i64,
    pub watched: bool,
    pub season: i64,
    pub episode: i64,
    pub show_id: String,
    pub season_id: String,
    pub genres: Vec<String>,
    /// `"MRAM-11"`, `"Tilgængelig til 21. dec"`, … — DR's own age and availability copy.
    pub rating: Option<String>,
    /// `"S5:E31 <name>"` as DR prints it under its own play button.
    pub label: Option<String>,
    /// How many episodes a season holds, when DR says. Used for "12 afsnit" copy.
    pub episode_count: i64,
}

impl Card {
    pub fn kind(&self) -> Kind {
        Kind::parse(&self.kind)
    }

    /// 0.0–1.0, or 0.0 when nothing is known.
    pub fn progress(&self) -> f32 {
        if self.duration_s <= 0 || self.resume_ms <= 0 {
            return 0.0;
        }
        (self.resume_ms as f32 / (self.duration_s as f32 * 1000.0)).clamp(0.0, 1.0)
    }

    pub fn resume_seconds(&self) -> i64 {
        if self.resume_ms <= 0 { 0 } else { self.resume_ms / 1000 }
    }

    /// The line under the title on a tile: `"S5:E31"` for an episode, the year for a film, the
    /// number of seasons for a show.
    pub fn kicker(&self) -> String {
        let numbered = self.season > 0 && self.episode > 0;
        match self.kind() {
            // **The numbers, not DR's label.** `PlayButtonExtraDetails` reads "S2026:E17 Klar! Læg an!
            // Skyd!" — the label repeats the episode's own title, and a kicker that repeats the line
            // above it is noise. The numbers are the part a viewer scans for.
            Kind::Episode if numbered => format!("S{}:E{}", self.season, self.episode),
            Kind::Episode => short_label(self.label.as_deref()),
            Kind::Season if self.season > 0 => format!("Sæson {}", self.season),
            _ => self.year.map(|y| y.to_string()).unwrap_or_default(),
        }
    }
}

/// The leading "S5:E31" of DR's label, when the numbering fields did not arrive.
fn short_label(label: Option<&str>) -> String {
    let Some(label) = label else {
        return String::new();
    };
    let mut out = String::new();
    for (i, c) in label.char_indices() {
        if i == 0 && c != 'S' {
            break;
        }
        if i > 0 && !(c.is_ascii_digit() || matches!(c, 'S' | 'E' | ':')) {
            break;
        }
        out.push(c);
    }
    if out.contains(":E") { out } else { String::new() }
}

/// Map one DR item. `watch` is the signed-in profile's history for that item, when there is one.
pub fn card(item: &Item, watch: Option<&Watch>) -> Card {
    let kind = Kind::parse(&item.kind);
    // A portrait tile wants 2:3 art. An episode's own art is 16:9, so its tile uses the show's
    // poster; a search result or an episode row draws `still` instead.
    let poster = if kind == Kind::Episode {
        sized(&item.images.poster, 480, 720)
    } else if !item.images.poster.is_empty() {
        sized(&item.images.poster, 480, 720)
    } else if !item.images.tile.is_empty() {
        sized(&item.images.tile, 480, 720)
    } else {
        sized(&item.images.wallpaper, 480, 720)
    };
    // The item's OWN landscape art, in preference order: an episode's still, a show's wallpaper, a
    // film's tile.
    let still = if !item.images.tile.is_empty() {
        sized(&item.images.tile, 960, 540)
    } else if !item.images.wallpaper.is_empty() {
        sized(&item.images.wallpaper, 960, 540)
    } else {
        sized(&item.images.poster, 960, 540)
    };
    let description = if item.description.trim().is_empty() {
        item.short_description.trim().to_string()
    } else {
        item.description.trim().to_string()
    };
    let (episode, season) = numbering(item);
    let resume_ms = match watch {
        Some(w) if !w.fully && w.position > 0 => w.position * 1000,
        _ => 0,
    };
    Card {
        id: item.id.clone(),
        kind: item.kind.clone(),
        title: if item.title.is_empty() { item.episode_name.clone() } else { item.title.clone() },
        episode_name: item.episode_name.clone(),
        description,
        poster,
        still,
        year: item.release_year.filter(|y| *y > 0),
        duration_s: item.duration,
        resume_ms,
        watched: matches!(watch, Some(w) if w.fully),
        season,
        episode,
        show_id: item.show_id.clone(),
        season_id: item.season_id.clone(),
        genres: item.genres.iter().map(|g| g.title().to_string()).collect(),
        rating: item.custom_str("ClassificationRating").map(str::to_string),
        label: item.custom_str("PlayButtonExtraDetails").map(str::to_string),
        episode_count: if item.episode_count > 0 {
            item.episode_count
        } else {
            item.available_episode_count
        },
    }
}

/// One shelf on the front page.
#[derive(Debug, Clone, Default)]
pub struct Shelf {
    pub id: String,
    pub title: String,
    pub items: Vec<Card>,
    /// The entry named a list instead of carrying items, and that list has not been fetched yet.
    /// Distinct from "carried items that were all filtered out", which must not be re-requested.
    pub pending: bool,
}

/// The whole front page, in DR's own order.
pub fn shelves(page: &api::Page, watches: &BTreeMap<String, Watch>) -> Vec<Shelf> {
    let mut shelves: Vec<Shelf> = Vec::with_capacity(page.entries.len());
    for entry in &page.entries {
        // A DR page entry either carries its items inline (a hand-picked shelf) or names a list to
        // fetch. `items` is filled in later for the second kind.
        let source = entry.list.as_ref();
        let items: Vec<Card> = source
            .map(|list| {
                list.items
                    .iter()
                    .filter(|item| wants_tile(item))
                    .map(|i| card(i, watches.get(&i.id)))
                    .collect()
            })
            .unwrap_or_default();
        // **The entry's own title, never the list's.** A list's title is CMS copy — "DRTV Hero Monday
        // 06.00", "Top text links + Jul (2)" — and a shelf with no title is a hand-picked row that DR
        // itself draws without a heading.
        let title = entry.title.clone();
        let id = source.map(|l| l.id.clone()).unwrap_or_else(|| entry.title.clone());
        let pending = source.is_some_and(|list| list.items.is_empty());
        if title.is_empty() && items.is_empty() && !pending {
            continue;
        }
        shelves.push(Shelf { id, title, items, pending });
    }
    shelves
}

/// Whether an item belongs on a poster shelf. DR mixes editorial entries into a list — `link` rows
/// are 16:9 category buttons with a marketing image and no runtime — and a link cropped into a 2:3
/// tile is a white smear. An item that simply lacks art still gets a tile: the placeholder draws its
/// initial, which is better than silently losing a title DR chose to show.
fn wants_tile(item: &api::Item) -> bool {
    matches!(item.kind.as_str(), "movie" | "program" | "show" | "series" | "season" | "episode")
}

/// The ids of the shelves that name a list must be fetched with `/lists?ids=…`.
pub fn pending_list_ids(shelves: &[Shelf]) -> Vec<String> {
    shelves.iter().filter(|s| s.pending && is_numeric_list(&s.id)).map(|s| s.id.clone()).collect()
}

fn is_numeric_list(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
}

/// Fill shelf items from a `/lists` reply, matching on list id.
pub fn fill_lists(
    shelves: &mut Vec<Shelf>,
    lists: &[api::List],
    watches: &BTreeMap<String, Watch>,
) {
    for list in lists {
        let items: Vec<Card> = list
            .items
            .iter()
            .filter(|item| wants_tile(item))
            .map(|i| card(i, watches.get(&i.id)))
            .collect();
        for shelf in shelves.iter_mut().filter(|s| s.id == list.id) {
            shelf.items = items.clone();
            shelf.pending = false;
        }
    }
    shelves.retain(|s| !s.items.is_empty());
}

/// Everything a detail page needs, from one expanded item read.
#[derive(Debug, Clone, Default)]
pub struct Detail {
    pub card: Card,
    pub seasons: Vec<Card>,
    pub episodes: Vec<Card>,
    /// The season currently selected in the season bar (its own card, when it has one).
    pub selected_season: i64,
}

pub fn detail(item: &Item, watches: &BTreeMap<String, Watch>) -> Detail {
    let mut seasons: Vec<Card> =
        item.seasons.items.iter().map(|i| card(i, watches.get(&i.id))).collect();
    seasons.sort_by_key(|c| c.season);
    let mut episodes: Vec<Card> =
        item.episodes.items.iter().map(|i| card(i, watches.get(&i.id))).collect();
    episodes.sort_by_key(|c| c.episode);
    let card = card(item, watches.get(&item.id));
    Detail { selected_season: card.season, card, seasons, episodes }
}

/// DR's own episode label carries the two numbers a tile and the detail page walk by: `"S5:E31 …"`,
/// and for a daily show `"S2026:E162 …"`. `episodeNumber` wins over the label when DR sends it — the
/// field is the item's, the label is copy DR prints for its own play button.
pub fn numbering(item: &Item) -> (i64, i64) {
    let labelled = item
        .custom_str("PlayButtonExtraDetails")
        .and_then(|l| l.strip_prefix('S'))
        .and_then(|rest| rest.split_once(":E"))
        .and_then(|(season, tail)| {
            let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
            Some((season.parse::<i64>().ok()?, digits.parse::<i64>().ok()?))
        });
    match Kind::parse(&item.kind) {
        Kind::Episode => {
            let (labelled_season, labelled_episode) = labelled.unwrap_or((0, 0));
            // The fields win over the label: they are the item's own, the label is copy DR prints
            // for its play button. A daily show's `seasonNumber` is the year (2026), which the label
            // also spells, so the two agree where both exist.
            let episode =
                if item.episode_number > 0 { item.episode_number } else { labelled_episode };
            let season = if item.season_number > 0 { item.season_number } else { labelled_season };
            (episode, season)
        }
        Kind::Season => (item.season_number, 0),
        _ => (0, 0),
    }
}

/// Rewrite DR's resize-CDN query so we fetch the pixels we will actually draw. The CDN resizes
/// server-side (`Width=`/`Height=`), which is cheaper than decoding a 4K JPEG for a 250-point tile.
pub fn sized(url: &str, w: u32, h: u32) -> String {
    fn replace(url: &str, key: &str, value: u32) -> String {
        match url.find(key) {
            None => url.to_string(),
            Some(pos) => {
                let digits_at = pos + key.len();
                let rest = &url[digits_at..];
                let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
                format!("{}{}{}", &url[..digits_at], value, &rest[end..])
            }
        }
    }
    if url.is_empty() {
        return String::new();
    }
    let out = replace(url, "Width=", w);
    replace(&out, "Height=", h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: &str) -> Item {
        serde_json::from_str(json).expect("fixture")
    }

    #[test]
    fn dr_spells_a_feature_program_and_that_is_a_movie_here() {
        let i = item(r#"{"id":"1","type":"program","title":"Borgen","duration":3300}"#);
        assert_eq!(card(&i, None).kind(), Kind::Movie);
        assert!(card(&i, None).kind().playable());
    }

    #[test]
    fn an_episode_tile_uses_the_shows_poster_and_its_own_still() {
        let i = item(
            r#"{"id":"2","type":"episode","title":"Del 1","showId":"s1","seasonNumber":2,
                "episodeNumber":3,"episodeName":"Del 1",
                "images":{"poster":"https://x/p.jpg?Width=800&Height=1200",
                          "tile":"https://x/t.jpg?Width=1920&Height=1080"}}"#,
        );
        let c = card(&i, None);
        assert_eq!(c.poster, "https://x/p.jpg?Width=480&Height=720");
        assert_eq!(c.still, "https://x/t.jpg?Width=960&Height=540");
        assert_eq!((c.season, c.episode), (2, 3));
        assert_eq!(c.kicker(), "S2:E3");
        assert_eq!(c.show_id, "s1");
    }

    #[test]
    fn a_daily_shows_four_digit_season_label_parses() {
        let i = item(
            r#"{"id":"3","type":"episode","title":"Aften","seasonNumber":2026,"episodeNumber":162}"#,
        );
        assert_eq!((card(&i, None).season, card(&i, None).episode), (2026, 162));
    }

    #[test]
    fn the_play_button_label_is_the_fallback_for_a_missing_episode_number() {
        let i = item(
            r#"{"id":"4","type":"episode","title":"Del 7","customFields":{"PlayButtonExtraDetails":"S5:E31 En aften"}}"#,
        );
        let c = card(&i, None);
        assert_eq!((c.season, c.episode), (5, 31));
    }

    #[test]
    fn progress_comes_from_seconds_on_the_wire_and_ms_in_the_card() {
        let i = item(r#"{"id":"5","type":"program","title":"X","duration":1000}"#);
        let w: Watch = serde_json::from_str(r#"{"position":250}"#).expect("watch");
        let c = card(&i, Some(&w));
        assert_eq!(c.resume_ms, 250_000);
        assert!((c.progress() - 0.25).abs() < f32::EPSILON);
        // A fully watched item reports no resume point; DR leaves `position` behind.
        let done: Watch =
            serde_json::from_str(r#"{"position":900,"isFullyWatched":true}"#).expect("watch");
        let c = card(&i, Some(&done));
        assert_eq!(c.resume_ms, 0);
        assert!(c.watched);
        assert!(c.progress() < f32::EPSILON);
    }

    #[test]
    fn a_short_description_is_the_fallback_blurb() {
        let i = item(r#"{"id":"6","type":"program","shortDescription":"Kort"}"#);
        assert_eq!(card(&i, None).description, "Kort");
    }

    #[test]
    fn a_show_shelf_entry_naming_a_list_is_fetched_later_and_filled_by_id() {
        let page: api::Page = serde_json::from_str(
            r#"{"entries":[
                 {"type":"list","title":"Fortsæt","list":{"id":"111"}},
                 {"type":"list","title":"Populært","list":{"id":"222","items":[{"id":"9","type":"program","title":"Y"}]}}
               ]}"#,
        )
        .expect("page");
        let mut shelves = shelves(&page, &BTreeMap::new());
        assert_eq!(pending_list_ids(&shelves), vec!["111".to_string()]);
        let lists: Vec<api::List> = serde_json::from_str(
            r#"[{"id":"111","items":[{"id":"7","type":"program","title":"Z"}]}]"#,
        )
        .expect("lists");
        fill_lists(&mut shelves, &lists, &BTreeMap::new());
        let fetched = shelves.iter().find(|s| s.id == "111").expect("shelf");
        assert_eq!(fetched.items.len(), 1);
        assert_eq!(fetched.items[0].title, "Z");
        assert!(pending_list_ids(&shelves).is_empty());
    }

    #[test]
    fn an_empty_list_is_dropped_rather_than_drawn_as_a_heading() {
        let page: api::Page = serde_json::from_str(
            r#"{"entries":[{"type":"list","title":"Tom","list":{"id":"333"}}]}"#,
        )
        .expect("page");
        let mut shelves = shelves(&page, &BTreeMap::new());
        fill_lists(&mut shelves, &[], &BTreeMap::new());
        assert!(shelves.is_empty());
    }

    #[test]
    fn sized_leaves_a_url_without_size_parameters_alone() {
        assert_eq!(sized("https://x/a.jpg", 100, 200), "https://x/a.jpg");
        assert_eq!(sized("", 100, 200), "");
        assert_eq!(
            sized("https://x/a.jpg?Width=10&Height=20&crop=1", 100, 200),
            "https://x/a.jpg?Width=100&Height=200&crop=1"
        );
    }
}
