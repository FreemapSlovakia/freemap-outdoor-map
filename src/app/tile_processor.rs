use sled::Batch;

use crate::app::tile_coord::TileCoord;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::ErrorKind,
    path::PathBuf,
    time::{Duration, SystemTime},
};

#[derive(Clone)]
pub(crate) struct VariantConfig {
    pub(crate) tile_cache_base_path: Option<PathBuf>,
    pub(crate) tile_index: Option<PathBuf>,
}

#[derive(Clone)]
pub(crate) struct TileProcessingConfig {
    pub(crate) variants: Vec<VariantConfig>,
    pub(crate) invalidate_min_zoom: u8,
}

struct VariantRuntime {
    tile_cache_base_path: Option<PathBuf>,
    db: Option<sled::Db>,
}

pub(crate) struct TileProcessor {
    variants: Vec<VariantRuntime>,
    invalidate_min_zoom: u8,
    invalidation_register: HashMap<TileCoord, SystemTime>,
    last_prune: SystemTime,
}

fn concatenate_merge(
    _key: &[u8],              // the key being merged
    old_value: Option<&[u8]>, // the previous value, if one existed
    merged_bytes: &[u8],      // the new bytes being merged in
) -> Option<Vec<u8>> {
    // set the new value, return None to delete
    let mut ret = old_value.map(|ov| ov.to_vec()).unwrap_or_default();

    ret.extend_from_slice(merged_bytes);

    Some(ret)
}

impl TileProcessor {
    pub(crate) fn new(config: TileProcessingConfig) -> Result<Self, sled::Error> {
        let mut variants = Vec::with_capacity(config.variants.len());

        for variant in config.variants {
            let db = variant.tile_index.map(sled::open).transpose()?;

            if let Some(ref db) = db {
                db.set_merge_operator(concatenate_merge);
            }

            variants.push(VariantRuntime {
                tile_cache_base_path: variant.tile_cache_base_path,
                db,
            });
        }

        Ok(Self {
            variants,
            invalidate_min_zoom: config.invalidate_min_zoom,
            invalidation_register: HashMap::new(),
            last_prune: SystemTime::now(),
        })
    }

    pub(crate) fn last_prune(&self) -> SystemTime {
        self.last_prune
    }

    pub(crate) fn set_last_prune(&mut self, now: SystemTime) {
        self.last_prune = now;
    }

    pub(crate) fn handle_save_tile(
        &mut self,
        data: Vec<u8>,
        coord: TileCoord,
        scale: f64,
        render_started_at: SystemTime,
        variant_index: usize,
    ) {
        if self.should_drop_save(coord, render_started_at) {
            return;
        }

        let Some(variant) = self.variants.get(variant_index) else {
            eprintln!("save tile for unknown variant index: {variant_index}");
            return;
        };

        let Some(tile_cache_base_path) = variant.tile_cache_base_path.as_ref() else {
            return;
        };

        self.append_index_entry(variant.db.as_ref(), coord, scale);

        let file_path = cached_tile_path(tile_cache_base_path, coord, scale);

        if let Some(parent) = file_path.parent()
            && let Err(err) = fs::create_dir_all(parent)
        {
            eprintln!("create tile dir failed: {err}");
        }

        if let Err(err) = fs::write(&file_path, data) {
            eprintln!("write tile failed: {err}");
        }
    }

    pub(crate) fn handle_invalidation(&mut self, coord: TileCoord, invalidated_at: SystemTime) {
        self.record_invalidation(coord, invalidated_at);

        for variant in &self.variants {
            let (Some(base_path), Some(db)) =
                (variant.tile_cache_base_path.as_ref(), variant.db.as_ref())
            else {
                continue;
            };

            let mut batch = Batch::default();

            self.remove_descendants(db, &mut batch, coord, base_path);

            let mut current = coord;
            loop {
                if current.zoom < self.invalidate_min_zoom {
                    break;
                }

                let Some(parent) = current.parent() else {
                    break;
                };

                current = parent;

                self.remove_exact(db, &mut batch, current, base_path);
            }

            if let Err(err) = db.apply_batch(batch) {
                eprintln!("failed to apply DB remove batch for {}: {err}", coord);
            }
        }
    }

    pub(crate) fn prune_invalidation_register(&mut self, now: SystemTime, ttl: Duration) {
        self.invalidation_register
            .retain(|_, ts| now.duration_since(*ts).unwrap_or(Duration::ZERO) <= ttl);
    }

    fn record_invalidation(&mut self, coord: TileCoord, invalidated_at: SystemTime) {
        let entry = self
            .invalidation_register
            .entry(coord)
            .or_insert(invalidated_at);

        if *entry < invalidated_at {
            *entry = invalidated_at;
        }
    }

    fn should_drop_save(&self, coord: TileCoord, render_started_at: SystemTime) -> bool {
        let mut coord = coord;

        loop {
            if let Some(invalidated_at) = self.invalidation_register.get(&coord)
                && render_started_at >= *invalidated_at
            {
                return true;
            }

            if let Some(parent) = coord.parent() {
                coord = parent;
            } else {
                break;
            }
        }

        false
    }

    fn append_index_entry(&self, db: Option<&sled::Db>, coord: TileCoord, scale: f64) {
        let Some(db) = db else {
            return;
        };

        let key: Vec<u8> = coord.into();

        if let Err(err) = db.merge(key, [scale.round() as u8; 1]) {
            eprint!("error merging tile {coord}: {err}")
        }
    }

    fn remove_descendants(
        &self,
        db: &sled::Db,
        batch: &mut Batch,
        coord: TileCoord,
        base_path: &std::path::Path,
    ) {
        let key: Vec<u8> = coord.into();

        for item in db.scan_prefix(key) {
            match item {
                Ok(entry) => {
                    let entry_coord = entry.0.as_ref().into();
                    self.remove_files(entry_coord, entry.1.as_ref(), base_path);
                    batch.remove(entry.0);
                }
                Err(err) => {
                    eprintln!("error scanning {coord}: {err}");
                }
            }
        }
    }

    fn remove_exact(
        &self,
        db: &sled::Db,
        batch: &mut Batch,
        coord: TileCoord,
        base_path: &std::path::Path,
    ) {
        let key: Vec<u8> = coord.into();

        let scales = match db.get(key.clone()) {
            Ok(Some(scales)) => scales,
            Ok(None) => return,
            Err(err) => {
                eprintln!("failed to get {} from DB: {err}", coord);
                return;
            }
        };

        self.remove_files(coord, scales.as_ref(), base_path);
        batch.remove(key);
    }

    fn remove_files(&self, coord: TileCoord, scales: &[u8], base_path: &std::path::Path) {
        let unique_scales: HashSet<u8> = scales.iter().copied().collect();

        for scale in unique_scales {
            let path = cached_tile_path(base_path, coord, scale as f64);

            if let Err(err) = fs::remove_file(&path)
                && err.kind() != ErrorKind::NotFound
            {
                eprintln!("failed to remove file {}: {err}", path.display());
            }
        }
    }
}

pub(crate) fn cached_tile_path(base: &std::path::Path, coord: TileCoord, scale: f64) -> PathBuf {
    let mut path = base.to_owned();
    path.push(coord.zoom.to_string());
    path.push(coord.x.to_string());
    path.push(format!("{}@{scale}.jpeg", coord.y));
    path
}
