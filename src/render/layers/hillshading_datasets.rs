use crate::render::{FeatureLineMaskCountries, HillshadingHierarchy};
use gdal::Dataset;
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};

const EVICT_AFTER: Duration = Duration::from_secs(10);

struct Slot {
    path: PathBuf,
    state: Mutex<SlotState>,
    returned: Condvar,
}

struct SlotState {
    idle: Vec<(Dataset, Instant)>,
    /// Idle plus checked-out handles.
    open: usize,
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
                    }),
                    returned: Condvar::new(),
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
                let mut state = slot
                    .state
                    .lock()
                    .expect("hillshading slot mutex not poisoned");

                let (keep, expired): (Vec<_>, Vec<_>) = std::mem::take(&mut state.idle)
                    .into_iter()
                    .partition(|(_, returned_at)| {
                        now.duration_since(*returned_at) <= EVICT_AFTER
                    });

                state.idle = keep;
                state.open -= expired.len();

                expired
            };

            // Closed outside the lock.
            drop(expired);
        }
    }

    /// Borrows an open handle, waiting while `max_open` handles are already in use.
    pub fn get(&self, name: &str) -> Option<DatasetGuard<'_>> {
        let slot = self.slots.get(name)?;

        {
            let mut state = slot
                .state
                .lock()
                .expect("hillshading slot mutex not poisoned");

            loop {
                // Newest first, so surplus handles stay idle long enough to be evicted.
                if let Some((dataset, _)) = state.idle.pop() {
                    return Some(DatasetGuard {
                        slot,
                        dataset: Some(dataset),
                    });
                }

                if state.open < self.max_open {
                    state.open += 1;
                    break;
                }

                state = slot
                    .returned
                    .wait(state)
                    .expect("hillshading slot mutex not poisoned");
            }
        }

        match Dataset::open(&slot.path) {
            Ok(dataset) => Some(DatasetGuard {
                slot,
                dataset: Some(dataset),
            }),
            Err(err) => {
                slot.state
                    .lock()
                    .expect("hillshading slot mutex not poisoned")
                    .open -= 1;

                slot.returned.notify_one();

                eprintln!(
                    "Error opening hillshading geotiff {}: {err}",
                    slot.path.display()
                );

                None
            }
        }
    }
}

/// Returns the handle to its pool on drop.
pub struct DatasetGuard<'a> {
    slot: &'a Slot,
    dataset: Option<Dataset>,
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
            self.slot
                .state
                .lock()
                .expect("hillshading slot mutex not poisoned")
                .idle
                .push((dataset, Instant::now()));

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
