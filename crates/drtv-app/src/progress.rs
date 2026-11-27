//! Where this machine left off in a title.
//!
//! DR keeps a position per profile, which is the right answer for a viewer who has signed in and
//! no answer at all for one who has not: the app's whole front page works anonymously, and until
//! now watching an episode anonymously left no trace anywhere. A short JSON file next to the
//! session fixes that. It is also, deliberately, the only thing here that survives deleting the
//! session: forgetting a sign-in should not forget a film.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Nothing under this many seconds is a position, it is a mistake or a shrug.
const MIN_SECONDS: i64 = 10;
/// Past this share of the title, the viewer has finished it and does not want to be offered the
/// last two minutes again. The same rule DR applies to its own watch marks.
const FINISHED: f64 = 0.95;
/// Old positions are dropped rather than kept forever: this is a convenience, not a history.
const KEEP: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    seconds: i64,
    at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Progress {
    #[serde(default)]
    entries: BTreeMap<String, Entry>,
}

impl Progress {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Save, best effort. A failed write costs a resume point, never the playback.
    pub fn save(&self, path: &Path) {
        let Ok(bytes) = serde_json::to_vec_pretty(self) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Write beside the target and rename, so a crash mid-write cannot leave half a file.
        let temp = path.with_extension("json.tmp");
        if std::fs::write(&temp, &bytes).is_ok() {
            let _ = std::fs::rename(&temp, path);
        }
    }

    /// The position to resume at, in seconds, if there is one worth resuming.
    pub fn get(&self, id: &str) -> Option<i64> {
        self.entries.get(id).map(|entry| entry.seconds).filter(|s| *s >= MIN_SECONDS)
    }

    /// Remember a position. Called with what mpv reports, so a viewer who watches to the end is
    /// forgotten rather than offered the credits again.
    pub fn set(&mut self, id: &str, seconds: f64, duration: f64) {
        if id.is_empty() {
            return;
        }
        let at = now();
        let seconds = seconds.round() as i64;
        if duration > 0.0 && seconds as f64 >= duration * FINISHED {
            self.clear(id);
            return;
        }
        if seconds < MIN_SECONDS {
            return;
        }
        self.entries.insert(id.to_string(), Entry { seconds, at });
        if self.entries.len() > KEEP {
            self.trim();
        }
    }

    pub fn clear(&mut self, id: &str) {
        self.entries.remove(id);
    }

    fn trim(&mut self) {
        let mut order: Vec<(String, u64)> =
            self.entries.iter().map(|(id, entry)| (id.clone(), entry.at)).collect();
        order.sort_by_key(|(_, at)| *at);
        for (id, _) in order.into_iter().take(self.entries.len() - KEEP) {
            self.entries.remove(&id);
        }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_real_position_and_drops_a_glance() {
        let mut progress = Progress::default();
        progress.set("a", 4.0, 1800.0);
        assert_eq!(progress.get("a"), None);
        progress.set("a", 61.7, 1800.0);
        assert_eq!(progress.get("a"), Some(62));
    }

    #[test]
    fn a_finished_title_is_forgotten() {
        let mut progress = Progress::default();
        progress.set("a", 700.0, 1000.0);
        assert_eq!(progress.get("a"), Some(700));
        progress.set("a", 980.0, 1000.0);
        assert_eq!(progress.get("a"), None, "95% is the end, not a position");
    }

    #[test]
    fn a_long_history_is_cut_back_to_a_bound() {
        let mut progress = Progress::default();
        for i in 0..(KEEP as i64 + 5) {
            progress.set(&format!("item{i}"), 60.0, 1800.0);
        }
        // Which five go is down to the clock; that the file cannot grow forever is the point.
        assert_eq!(progress.entries.len(), KEEP);
    }

    #[test]
    fn round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("drtv-progress-{}", std::process::id()));
        let path = dir.join("progress.json");
        let mut progress = Progress::default();
        progress.set("abc", 321.0, 1800.0);
        progress.save(&path);

        let read = Progress::load(&path);
        assert_eq!(read.get("abc"), Some(321));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
