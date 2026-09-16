use crate::render::{
    FeatureLineMaskCountries, HillshadingHierarchy, layers::hillshading_footprint::Footprint,
};
use gdal::Dataset;
use geo::Rect;
use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{
        Condvar, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const EVICT_AFTER: Duration = Duration::from_secs(10);

/// A wait this long is logged as it happens: every handle of the dataset stayed busy for it.
const SLOW_WAIT: Duration = Duration::from_secs(1);

/// Shorter holds read nothing worth timing. A dataset with a footprint stops missing
/// the tile this expensively — it is rejected before the checkout — but the first call
/// per dataset, and every dataset without a footprint, still lands here.
const MEANINGFUL_HOLD: Duration = Duration::from_millis(10);

const POISONED: &str = "hillshading slot mutex not poisoned";

struct Slot {
    path: PathBuf,
    state: Mutex<SlotState>,
    returned: Condvar,
    /// Built on the first checkout of the dataset and kept for the process' life —
    /// ~8 KB, unlike the tile index the handles carry, so it outlives eviction.
    /// `None` inside means the dataset has no footprint and is never skipped.
    footprint: OnceLock<Option<Footprint>>,
    /// Held only while the footprint is being built, so the workers that arrive during
    /// it wait rather than each building their own.
    building: Mutex<()>,
    /// Counted outside `state`: a skipped dataset must not touch the slot mutex.
    skipped: AtomicU64,
}

struct SlotState {
    idle: Vec<(Dataset, Instant)>,
    /// Idle plus checked-out handles.
    open: usize,
    stats: SlotStats,
}

/// Counters since the last [`HillshadingDatasets::log_stats`].
#[derive(Default)]
struct SlotStats {
    checkouts: u64,
    waits: u64,
    wait_total: Duration,
    wait_max: Duration,
    hold_max: Duration,
    in_use_peak: usize,
    opened: u64,
    evicted: u64,
}

/// Every open handle keeps the file's tile index in memory (up to ~1 GB for the
/// largest countries), so handles are shared by all workers and capped per dataset.
pub struct HillshadingDatasets {
    /// Dataset names that may be opened. Any request for a name outside this set is
    /// treated as "no dataset" so callers can hand in codes (e.g. hardcoded country
    /// lists) that aren't part of the configured hillshading hierarchy.
    slots: HashMap<String, Slot>,
    max_open: usize,
}

impl HillshadingDatasets {
    pub fn new(base: impl AsRef<Path>, allowed: HashSet<String>, max_open: usize) -> Self {
        let base = base.as_ref();

        let slots = allowed
            .into_iter()
            .map(|name| {
                let slot = Slot {
                    path: base.join(&name).join("final.tif"),
                    state: Mutex::new(SlotState {
                        idle: Vec::new(),
                        open: 0,
                        stats: SlotStats::default(),
                    }),
                    returned: Condvar::new(),
                    footprint: OnceLock::new(),
                    building: Mutex::new(()),
                    skipped: AtomicU64::new(0),
                };

                (name, slot)
            })
            .collect();

        Self {
            slots,
            max_open: max_open.max(1),
        }
    }

    pub fn evict_unused(&self) {
        let now = Instant::now();

        for slot in self.slots.values() {
            let expired = {
                let mut state = slot.state.lock().expect(POISONED);

                let (keep, expired): (Vec<_>, Vec<_>) = std::mem::take(&mut state.idle)
                    .into_iter()
                    .partition(|(_, returned_at)| {
                        now.duration_since(*returned_at) <= EVICT_AFTER
                    });

                state.idle = keep;
                state.open -= expired.len();
                state.stats.evicted += expired.len() as u64;

                expired
            };

            // Closed outside the lock.
            drop(expired);
        }
    }

    /// Borrows an open handle for a tile, waiting while `max_open` handles are already
    /// in use. Returns `None` when the dataset has no data under `bbox`, before any
    /// waiting or opening happens — over the Netherlands that is about half the calls.
    pub fn get(&self, name: &str, bbox: &Rect<f64>) -> Option<DatasetGuard<'_>> {
        let slot = self.slots.get(name)?;

        // The footprint is read from the dataset, so the first call for one has to check
        // a handle out; it then hands that same handle on to the read.
        let guard = if slot.footprint.get().is_some() {
            None
        } else {
            let guard = self.checkout(slot, name)?;

            // Serialised, and re-checked inside: every worker reaching a cold dataset
            // together would otherwise read and fold the same overview, once each.
            // Held over the read, not stored in it, so a failed open is retried rather
            // than remembered as "no footprint".
            let building = slot.building.lock().expect(POISONED);

            if slot.footprint.get().is_none() {
                let _ = slot.footprint.set(Footprint::build(&guard, name));
            }

            drop(building);

            Some(guard)
        };

        if let Some(Some(footprint)) = slot.footprint.get()
            && !footprint.covers(bbox)
        {
            slot.skipped.fetch_add(1, Ordering::Relaxed);

            return None;
        }

        guard.or_else(|| self.checkout(slot, name))
    }

    fn checkout<'a>(&'a self, slot: &'a Slot, name: &str) -> Option<DatasetGuard<'a>> {
        let mut waiting_since = None;

        {
            let mut state = slot.state.lock().expect(POISONED);

            loop {
                // Newest first, so surplus handles stay idle long enough to be evicted.
                if let Some((dataset, _)) = state.idle.pop() {
                    record_checkout(&mut state, waiting_since, name, self.max_open);

                    return Some(DatasetGuard {
                        slot,
                        dataset: Some(dataset),
                        taken_at: Instant::now(),
                    });
                }

                if state.open < self.max_open {
                    state.open += 1;
                    record_checkout(&mut state, waiting_since, name, self.max_open);
                    break;
                }

                waiting_since.get_or_insert_with(Instant::now);

                state = slot.returned.wait(state).expect(POISONED);
            }
        }

        match Dataset::open(&slot.path) {
            Ok(dataset) => {
                slot.state.lock().expect(POISONED).stats.opened += 1;

                Some(DatasetGuard {
                    slot,
                    dataset: Some(dataset),
                    taken_at: Instant::now(),
                })
            }
            Err(err) => {
                slot.state.lock().expect(POISONED).open -= 1;

                slot.returned.notify_one();

                eprintln!(
                    "Error opening hillshading geotiff {}: {err}",
                    slot.path.display()
                );

                None
            }
        }
    }

    /// Prints the counters of every dataset used since the previous call and resets them.
    pub fn log_stats(&self, interval: Duration) {
        let mut names: Vec<&String> = self.slots.keys().collect();
        names.sort();

        let mut line = String::new();
        let mut total_checkouts = 0;
        let mut total_skipped = 0;

        for name in names {
            let slot = &self.slots[name];

            let (stats, open, idle) = {
                let mut state = slot.state.lock().expect(POISONED);

                let in_use = state.open - state.idle.len();
                let stats = std::mem::take(&mut state.stats);
                state.stats.in_use_peak = in_use;

                (stats, state.open, state.idle.len())
            };

            let skipped = slot.skipped.swap(0, Ordering::Relaxed);

            total_checkouts += stats.checkouts;
            total_skipped += skipped;

            if stats.waits == 0 && stats.hold_max < MEANINGFUL_HOLD && skipped == 0 {
                continue;
            }

            write!(
                line,
                " {name}[checkouts={} skipped={skipped} waits={} wait_total={:.1}s wait_max={:.2}s hold_max={:.2}s peak={}/{} open={open} idle={idle} opened={} evicted={}]",
                stats.checkouts,
                stats.waits,
                stats.wait_total.as_secs_f64(),
                stats.wait_max.as_secs_f64(),
                stats.hold_max.as_secs_f64(),
                stats.in_use_peak,
                self.max_open,
                stats.opened,
                stats.evicted,
            )
            .expect("writing to a String");
        }

        if !line.is_empty() || total_skipped > 0 {
            println!(
                "hillshading pool, last {}s: checkouts={total_checkouts} skipped={total_skipped}{line}",
                interval.as_secs()
            );
        }
    }
}

fn record_checkout(
    state: &mut SlotState,
    waiting_since: Option<Instant>,
    name: &str,
    max_open: usize,
) {
    let in_use = state.open - state.idle.len();
    let stats = &mut state.stats;

    stats.checkouts += 1;
    stats.in_use_peak = stats.in_use_peak.max(in_use);

    if let Some(since) = waiting_since {
        let waited = since.elapsed();

        stats.waits += 1;
        stats.wait_total += waited;
        stats.wait_max = stats.wait_max.max(waited);

        if waited >= SLOW_WAIT {
            eprintln!(
                "hillshading {name}: waited {:.1}s for a handle ({in_use}/{max_open} in use)",
                waited.as_secs_f64()
            );
        }
    }
}

/// Returns the handle to its pool on drop.
pub struct DatasetGuard<'a> {
    slot: &'a Slot,
    dataset: Option<Dataset>,
    taken_at: Instant,
}

impl Deref for DatasetGuard<'_> {
    type Target = Dataset;

    fn deref(&self) -> &Dataset {
        self.dataset.as_ref().expect("dataset is only taken on drop")
    }
}

impl Drop for DatasetGuard<'_> {
    fn drop(&mut self) {
        if let Some(dataset) = self.dataset.take() {
            let held = self.taken_at.elapsed();

            let mut state = self.slot.state.lock().expect(POISONED);
            state.stats.hold_max = state.stats.hold_max.max(held);
            state.idle.push((dataset, Instant::now()));
            drop(state);

            self.slot.returned.notify_one();
        }
    }
}

/// Create a lazily-loading dataset pool restricted to the hillshadings referenced by
/// `hierarchy` (plus the `_` global fallback) and by `feature_line_mask_countries`, whose
/// masks may come from countries that are not shaded themselves. Every `better` code is
/// validated to also be a `country` key, so the `country` keys cover the full set of
/// datasets the hierarchy references. Names outside this set are never opened from disk.
pub fn load_hillshading_datasets(
    base: impl AsRef<Path>,
    hierarchy: &HillshadingHierarchy,
    feature_line_mask_countries: Option<&FeatureLineMaskCountries>,
    max_open: usize,
) -> HillshadingDatasets {
    let mut allowed: HashSet<String> = hierarchy
        .entries()
        .iter()
        .map(|entry| entry.country.to_string())
        .collect();

    // Global fallback dataset used where no country mask covers the tile.
    allowed.insert("_".to_string());

    if let Some(feature_line_mask_countries) = feature_line_mask_countries {
        allowed.extend(feature_line_mask_countries.countries().iter().cloned());
    }

    HillshadingDatasets::new(base, allowed, max_open)
}
