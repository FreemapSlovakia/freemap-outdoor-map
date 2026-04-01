use gdal::Dataset;
use std::{
    collections::{HashMap, hash_map::Entry},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const DATASET_PATHS: [(&str, &str); 10] = [
    ("sk", "sk/final.tif"),
    ("cz", "cz/final.tif"),
    ("at", "at/final.tif"),
    ("pl", "pl/final.tif"),
    ("it", "it/final.tif"),
    ("ch", "ch/final.tif"),
    ("si", "si/final.tif"),
    ("fr", "fr/final.tif"),
    ("no", "no/final.tif"),
    ("_", "final.tif"),
];

const EVICT_AFTER: Duration = Duration::from_secs(10);

struct CachedDataset {
    dataset: Dataset,
    last_used_at: Instant,
}

pub struct HillshadingDatasets {
    base: PathBuf,
    datasets: HashMap<String, CachedDataset>,
}

impl HillshadingDatasets {
    pub fn new(base: impl AsRef<Path>) -> Self {
        Self {
            base: base.as_ref().to_path_buf(),
            datasets: HashMap::new(),
        }
    }

    pub fn evict_unused(&mut self) {
        let now = Instant::now();

        self.datasets
            .retain(|_, cached| now.duration_since(cached.last_used_at) <= EVICT_AFTER);
    }

    pub fn get(&mut self, name: &str) -> Option<&Dataset> {
        match self.datasets.entry(name.to_string()) {
            Entry::Occupied(occ) => Some(&occ.into_mut().dataset),
            Entry::Vacant(vac) => {
                let Some(path) = dataset_path(name) else {
                    eprintln!("Unknown hillshading dataset key: {name}");
                    return None;
                };

                let full_path = self.base.join(path);

                match Dataset::open(&full_path) {
                    Ok(dataset) => {
                        let entry = vac.insert(CachedDataset {
                            dataset,
                            last_used_at: Instant::now(),
                        });
                        Some(&entry.dataset)
                    }
                    Err(err) => {
                        eprintln!(
                            "Error opening hillshading geotiff {}: {}",
                            full_path.display(),
                            err
                        );
                        None
                    }
                }
            }
        }
    }

    pub fn record_use(&mut self, name: &str) {
        if let Some(entry) = self.datasets.get_mut(name) {
            entry.last_used_at = Instant::now();
        }
    }
}

fn dataset_path(name: &str) -> Option<&'static str> {
    DATASET_PATHS
        .iter()
        .find(|(dataset_name, _)| dataset_name == &name)
        .map(|(_, path)| *path)
}

pub fn load_hillshading_datasets(base: impl AsRef<Path>) -> HillshadingDatasets {
    HillshadingDatasets::new(base)
}
