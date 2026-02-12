use sled::Batch;

use crate::app::tile_coord::TileCoord;
use std::{
    collections::HashMap,
    fs,
    io::ErrorKind,
    path::PathBuf,
    time::{Duration, SystemTime},
};

#[derive(Clone)]
pub(crate) struct TileProcessingConfig {
    pub(crate) tile_cache_base_path: PathBuf,
    pub(crate) tile_index: Option<PathBuf>,
    pub(crate) invalidate_min_zoom: u8,
}

pub(crate) struct TileProcessor {
    config: TileProcessingConfig,
    invalidation_register: HashMap<TileCoord, SystemTime>,
    last_prune: SystemTime,
    db: Option<sled::Db>,
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
        let db = config.tile_index.clone().map(sled::open).transpose()?;

        if let Some(ref db) = db {
            db.set_merge_operator(concatenate_merge);
        }

        Ok(Self {
            config,
            invalidation_register: HashMap::new(),
            last_prune: SystemTime::now(),
            db,
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
    ) {
        if self.should_drop_save(coord, render_started_at) {
            return;
        }

        self.append_index_entry(coord, scale);

        let file_path = cached_tile_path(&self.config.tile_cache_base_path, coord, scale);

        if let Some(parent) = file_path.parent()
            && let Err(err) = fs::create_dir_all(parent)
        {
            eprintln!("create tile dir failed: {err}");
        }

        if let Err(err) = fs::write(&file_path, data) {
            eprintln!("write tile failed: {err}");
        }
    }

    fn remove(&self, batch: &mut Batch, coord: TileCoord, scales: impl AsRef<[u8]>) {
        for scale in scales.as_ref() {
            let path = cached_tile_path(&self.config.tile_cache_base_path, coord, *scale as f64);

            if let Err(err) = fs::remove_file(&path)
                && err.kind() != ErrorKind::NotFound
            {
                eprintln!("failed to remove file {}: {err}", path.display());
            }
        }

        let key: Vec<u8> = coord.into();

        batch.remove(key)
    }

    pub(crate) fn handle_invalidation(&mut self, coord: TileCoord, invalidated_at: SystemTime) {
        self.record_invalidation(coord, invalidated_at);

        let Some(ref db) = self.db else {
            return;
        };

        let key: Vec<u8> = coord.into();

        let mut batch = Batch::default();

        for item in db.scan_prefix(key) {
            match item {
                Ok(entry) => {
                    let coord = entry.0.as_ref().into();

                    self.remove(&mut batch, coord, entry.1);
                }
                Err(err) => {
                    eprint!("error scanning {coord}: {err}");
                    continue;
                }
            }
        }

        let mut coord = coord;

        loop {
            if coord.zoom < self.config.invalidate_min_zoom {
                break;
            }

            let Some(parent) = coord.parent() else {
                break;
            };

            coord = parent;

            let key: Vec<u8> = coord.into();

            let scales = match db.get(key) {
                Ok(Some(scales)) => scales,
                Ok(None) => continue,
                Err(err) => {
                    eprintln!("failed to get {} from DB: {err}", coord);

                    continue;
                }
            };

            self.remove(&mut batch, coord, scales);
        }

        if let Err(err) = db.apply_batch(batch) {
            eprintln!("failed to apply DB remove batch for {}: {err}", coord);
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

    fn append_index_entry(&self, coord: TileCoord, scale: f64) {
        let Some(ref db) = self.db else {
            return;
        };

        let key: Vec<u8> = coord.into();

        if let Err(err) = db.merge(key, [scale.round() as u8; 1]) {
            eprint!("error merging tile {coord}: {err}")
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
