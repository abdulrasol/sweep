//! Freed-space history (PLAN 4.5).
//!
//! Every removal is appended as one JSON line to `history.jsonl` in the app
//! data folder. Only counts and sizes are stored: no paths and no project
//! names. The file is append-only, so a crash never corrupts older lines, and
//! unreadable lines are skipped on load.

use crate::cleanup_engine::CleanupItem;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const FILE_NAME: &str = "history.jsonl";
const TMP_NAME: &str = "history.jsonl.tmp";
/// Above this many lines the file is rewritten with only the newest entries.
const MAX_LINES: usize = 20_000;
const KEEP_LINES: usize = 10_000;
/// Counting lines is only worth the read once the file is about this big.
const CHECK_SIZE: u64 = 2_000_000;
const DAY_SECS: i64 = 86_400;
/// Length of the recent window shown on the History screen, in UTC days.
const RECENT_DAYS: i64 = 30;

/// One removal that freed (or moved to the Trash) some bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix seconds.
    pub ts: i64,
    pub module: String,
    pub file_type: String,
    pub category: String,
    pub bytes: u64,
    /// True when the item went to the Trash: the space is not free until the
    /// Trash is emptied.
    pub trashed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ModuleTotals {
    pub module: String,
    pub freed: u64,
    pub trashed: u64,
    pub count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DayTotals {
    /// UTC date, `YYYY-MM-DD`.
    pub day: String,
    pub freed: u64,
    pub trashed: u64,
    pub count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HistorySummary {
    pub total_freed: u64,
    pub total_trashed: u64,
    pub items_removed: u64,
    pub first_ts: Option<i64>,
    pub last_30_days_freed: u64,
    pub by_module: Vec<ModuleTotals>,
    pub by_day: Vec<DayTotals>,
}

/// Running totals for one group of entries.
#[derive(Default, Clone, Copy)]
struct Acc {
    freed: u64,
    trashed: u64,
    count: u64,
}

impl Acc {
    fn add(&mut self, entry: &Entry) {
        if entry.trashed {
            self.trashed = self.trashed.saturating_add(entry.bytes);
        } else {
            self.freed = self.freed.saturating_add(entry.bytes);
        }
        self.count += 1;
    }
}

/// Current time in unix seconds. The only clock read in this module.
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// The history entry for one removal, or `None` when nothing was freed.
/// `outcome` is `Ok((bytes, trashed))` for a removal, or `Err(freed)` for a
/// failed one that still freed `freed` bytes before it stopped.
pub fn entry_for(item: &CleanupItem, outcome: Result<(u64, bool), u64>, ts: i64) -> Option<Entry> {
    let (bytes, trashed) = match outcome {
        Ok((bytes, trashed)) => (bytes, trashed),
        Err(freed) => (freed, false),
    };
    if bytes == 0 {
        return None;
    }
    Some(Entry {
        ts,
        module: item.module.clone(),
        file_type: item.file_type.clone(),
        category: item.category.clone(),
        bytes,
        trashed,
    })
}

/// Append one entry. A failure here must never fail a removal, so callers use
/// [`record`].
pub fn append(dir: &Path, entry: &Entry) -> io::Result<()> {
    append_with_limits(dir, entry, MAX_LINES, KEEP_LINES, CHECK_SIZE)
}

fn append_with_limits(
    dir: &Path,
    entry: &Entry,
    max_lines: usize,
    keep_lines: usize,
    check_size: u64,
) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join(FILE_NAME);
    let mut line = serde_json::to_string(entry).map_err(io::Error::other)?;
    line.push('\n');
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(line.as_bytes())?;
    drop(file);

    let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size >= check_size {
        compact(dir, max_lines, keep_lines)?;
    }
    Ok(())
}

/// Keep only the newest `keep_lines` lines once the file holds more than
/// `max_lines`. Written to a temporary file and renamed into place.
fn compact(dir: &Path, max_lines: usize, keep_lines: usize) -> io::Result<()> {
    let path = dir.join(FILE_NAME);
    let bytes = fs::read(&path)?;
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() <= max_lines {
        return Ok(());
    }
    let keep = &lines[lines.len() - keep_lines.min(lines.len())..];
    let tmp = dir.join(TMP_NAME);
    let mut out = keep.join("\n");
    out.push('\n');
    fs::write(&tmp, out)?;
    fs::rename(&tmp, &path)
}

/// Save an entry; a failure is logged and otherwise ignored.
pub fn record(dir: &Path, entry: &Entry) {
    if let Err(e) = append(dir, entry) {
        eprintln!("Sweep: could not save the cleanup history: {e}");
    }
}

/// All readable entries, oldest first. A missing file is an empty history and
/// blank or corrupt lines are skipped.
pub fn load(dir: &Path) -> Vec<Entry> {
    let Ok(bytes) = fs::read(dir.join(FILE_NAME)) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| serde_json::from_str::<Entry>(line.trim()).ok())
        .collect()
}

/// Delete the whole history. A history that does not exist is already clear.
pub fn clear(dir: &Path) -> io::Result<()> {
    let _ = fs::remove_file(dir.join(TMP_NAME));
    match fs::remove_file(dir.join(FILE_NAME)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Totals for the History screen. `now` is injected so the result is a pure
/// function of its inputs.
pub fn summarize(entries: &[Entry], now: i64) -> HistorySummary {
    let today = now.div_euclid(DAY_SECS);
    let first_day = today - (RECENT_DAYS - 1);

    let mut total = Acc::default();
    let mut recent_freed: u64 = 0;
    let mut first_ts: Option<i64> = None;
    let mut modules: HashMap<String, Acc> = HashMap::new();
    let mut days: BTreeMap<i64, Acc> = BTreeMap::new();

    for entry in entries {
        total.add(entry);
        first_ts = Some(first_ts.map_or(entry.ts, |t| t.min(entry.ts)));
        modules.entry(entry.module.clone()).or_default().add(entry);

        let day = entry.ts.div_euclid(DAY_SECS);
        if (first_day..=today).contains(&day) {
            days.entry(day).or_default().add(entry);
            if !entry.trashed {
                recent_freed = recent_freed.saturating_add(entry.bytes);
            }
        }
    }

    let mut by_module: Vec<ModuleTotals> = modules
        .into_iter()
        .map(|(module, acc)| ModuleTotals {
            module,
            freed: acc.freed,
            trashed: acc.trashed,
            count: acc.count,
        })
        .collect();
    by_module.sort_by(|a, b| {
        b.freed
            .saturating_add(b.trashed)
            .cmp(&a.freed.saturating_add(a.trashed))
            .then_with(|| a.module.cmp(&b.module))
    });

    let by_day = days
        .into_iter()
        .map(|(day, acc)| DayTotals {
            day: day_string(day),
            freed: acc.freed,
            trashed: acc.trashed,
            count: acc.count,
        })
        .collect();

    HistorySummary {
        total_freed: total.freed,
        total_trashed: total.trashed,
        items_removed: total.count,
        first_ts,
        last_30_days_freed: recent_freed,
        by_module,
        by_day,
    }
}

/// `YYYY-MM-DD` for a count of days since 1970-01-01 (UTC).
fn day_string(days_since_epoch: i64) -> String {
    let (year, month, day) = civil_from_days(days_since_epoch);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup_engine::Safety;
    use tempfile::TempDir;

    /// 2026-10-20 12:00:00 UTC.
    const NOW: i64 = 1_792_497_600;

    fn entry(ts: i64, module: &str, bytes: u64, trashed: bool) -> Entry {
        Entry {
            ts,
            module: module.to_string(),
            file_type: "Test".to_string(),
            category: "Build Artifacts".to_string(),
            bytes,
            trashed,
        }
    }

    fn item(module: &str) -> CleanupItem {
        CleanupItem {
            id: "id".to_string(),
            path: "/tmp/never-stored".to_string(),
            file_type: "Rust".to_string(),
            category: "Build Output".to_string(),
            group: "Projects".to_string(),
            module: module.to_string(),
            size_bytes: 10,
            status: Safety::Safe,
            description: String::new(),
            blocked_by: None,
            last_active: None,
        }
    }

    #[test]
    fn append_then_load_keeps_order() {
        let d = TempDir::new().unwrap();
        let first = entry(100, "node", 5, false);
        let second = entry(200, "rust", 7, true);
        append(d.path(), &first).unwrap();
        append(d.path(), &second).unwrap();
        assert_eq!(load(d.path()), vec![first, second]);
    }

    #[test]
    fn append_creates_the_data_folder() {
        let d = TempDir::new().unwrap();
        let nested = d.path().join("a/b");
        append(&nested, &entry(1, "node", 1, false)).unwrap();
        assert_eq!(load(&nested).len(), 1);
    }

    #[test]
    fn load_skips_blank_and_corrupt_lines() {
        let d = TempDir::new().unwrap();
        let good = entry(100, "node", 5, false);
        append(d.path(), &good).unwrap();
        let path = d.path().join(FILE_NAME);
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"\n{not json}\n{\"ts\":1}\n").unwrap();
        drop(file);
        let last = entry(300, "rust", 9, false);
        append(d.path(), &last).unwrap();
        assert_eq!(load(d.path()), vec![good, last]);
    }

    #[test]
    fn load_of_a_missing_file_is_empty() {
        let d = TempDir::new().unwrap();
        assert!(load(d.path()).is_empty());
        assert!(load(&d.path().join("missing")).is_empty());
    }

    #[test]
    fn compaction_keeps_the_newest_entries() {
        let d = TempDir::new().unwrap();
        for i in 0..6 {
            // max 5 lines, keep 3, check on every append.
            append_with_limits(d.path(), &entry(i, "node", 1, false), 5, 3, 0).unwrap();
        }
        let kept: Vec<i64> = load(d.path()).iter().map(|e| e.ts).collect();
        assert_eq!(kept, vec![3, 4, 5]);
        assert!(!d.path().join(TMP_NAME).exists());

        append_with_limits(d.path(), &entry(6, "node", 1, false), 5, 3, 0).unwrap();
        let kept: Vec<i64> = load(d.path()).iter().map(|e| e.ts).collect();
        assert_eq!(kept, vec![3, 4, 5, 6]);
    }

    #[test]
    fn compaction_leaves_a_small_file_alone() {
        let d = TempDir::new().unwrap();
        for i in 0..3 {
            append_with_limits(d.path(), &entry(i, "node", 1, false), 5, 3, 0).unwrap();
        }
        assert_eq!(load(d.path()).len(), 3);
    }

    #[test]
    fn clear_removes_the_history_and_tolerates_a_missing_one() {
        let d = TempDir::new().unwrap();
        clear(d.path()).unwrap();
        append(d.path(), &entry(1, "node", 1, false)).unwrap();
        clear(d.path()).unwrap();
        assert!(load(d.path()).is_empty());
        assert!(!d.path().join(FILE_NAME).exists());
    }

    #[test]
    fn summarize_separates_freed_from_trashed() {
        let entries = vec![
            entry(NOW - 100, "node", 100, false),
            entry(NOW - 50, "node", 40, true),
            entry(NOW - 10, "rust", 300, false),
        ];
        let s = summarize(&entries, NOW);
        assert_eq!(s.total_freed, 400);
        assert_eq!(s.total_trashed, 40);
        assert_eq!(s.items_removed, 3);
        assert_eq!(s.first_ts, Some(NOW - 100));
        assert_eq!(s.last_30_days_freed, 400);
    }

    #[test]
    fn summarize_orders_modules_by_size_then_name() {
        let entries = vec![
            entry(NOW, "node", 100, false),
            entry(NOW, "rust", 500, false),
            entry(NOW, "python", 100, true),
            entry(NOW, "docker", 100, false),
        ];
        let s = summarize(&entries, NOW);
        let order: Vec<&str> = s.by_module.iter().map(|m| m.module.as_str()).collect();
        assert_eq!(order, vec!["rust", "docker", "node", "python"]);
        assert_eq!(s.by_module[3].trashed, 100);
        assert_eq!(s.by_module[3].freed, 0);
    }

    #[test]
    fn summarize_last_30_days_excludes_older_entries() {
        let old = NOW - 31 * DAY_SECS;
        let edge_in = NOW - 29 * DAY_SECS;
        let entries = vec![
            entry(old, "node", 1_000, false),
            entry(edge_in, "node", 10, false),
            entry(NOW, "node", 1, false),
        ];
        let s = summarize(&entries, NOW);
        assert_eq!(s.total_freed, 1_011);
        assert_eq!(s.last_30_days_freed, 11);
        assert_eq!(s.by_day.len(), 2);
    }

    #[test]
    fn summarize_by_day_is_sorted_and_grouped_by_utc_date() {
        // 2026-10-19 23:59:59 and 2026-10-20 00:00:00 UTC are different days.
        let midnight: i64 = 1_792_454_400;
        assert_eq!(day_string(midnight.div_euclid(DAY_SECS)), "2026-10-20");
        let entries = vec![
            entry(midnight, "node", 2, false),
            entry(midnight - 1, "node", 1, false),
            entry(midnight + 60, "rust", 4, true),
        ];
        let s = summarize(&entries, NOW);
        assert_eq!(
            s.by_day,
            vec![
                DayTotals {
                    day: "2026-10-19".to_string(),
                    freed: 1,
                    trashed: 0,
                    count: 1
                },
                DayTotals {
                    day: "2026-10-20".to_string(),
                    freed: 2,
                    trashed: 4,
                    count: 2
                },
            ]
        );
    }

    #[test]
    fn summarize_of_nothing_is_empty() {
        let s = summarize(&[], NOW);
        assert_eq!(s, HistorySummary::default());
    }

    #[test]
    fn civil_dates_are_right() {
        assert_eq!(day_string(0), "1970-01-01");
        assert_eq!(day_string(11_016), "2000-02-29");
        assert_eq!(day_string(NOW.div_euclid(DAY_SECS)), "2026-10-20");
        assert_eq!(day_string(-1), "1969-12-31");
    }

    #[test]
    fn entry_for_covers_every_outcome() {
        let it = item("rust");
        let removed = entry_for(&it, Ok((500, false)), 7).unwrap();
        assert_eq!(removed.bytes, 500);
        assert!(!removed.trashed);
        assert_eq!(removed.module, "rust");
        assert_eq!(removed.ts, 7);

        let trashed = entry_for(&it, Ok((500, true)), 7).unwrap();
        assert!(trashed.trashed);

        let partial = entry_for(&it, Err(120), 7).unwrap();
        assert_eq!(partial.bytes, 120);
        assert!(!partial.trashed);

        assert_eq!(entry_for(&it, Err(0), 7), None);
        assert_eq!(entry_for(&it, Ok((0, false)), 7), None);
    }

    #[test]
    fn a_failing_write_never_panics() {
        let d = TempDir::new().unwrap();
        // A regular file where the data folder should be.
        let blocker = d.path().join("blocker");
        fs::write(&blocker, b"x").unwrap();
        assert!(append(&blocker, &entry(1, "node", 1, false)).is_err());
        record(&blocker, &entry(1, "node", 1, false));
    }
}
