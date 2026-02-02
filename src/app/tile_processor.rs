use crate::app::tile_coord::{TileCoord, TileCoordParseError};
use std::{
    collections::HashMap,
    fs,
    io::{ErrorKind, Read, Write},
    path::PathBuf,
    time::{Duration, SystemTime},
};

#[derive(Clone)]
pub(crate) struct TileProcessingConfig {
    pub(crate) tile_cache_root: PathBuf,
    pub(crate) index_zoom: u8,
    pub(crate) max_zoom: u8,
    pub(crate) invalidate_min_zoom: u8,
}

pub(crate) struct TileProcessor {
    config: TileProcessingConfig,
    invalidation_register: HashMap<TileCoord, SystemTime>,
    last_prune: SystemTime,
}

impl TileProcessor {
    pub(crate) fn new(config: TileProcessingConfig) -> Self {
        Self {
            config,
            invalidation_register: HashMap::new(),
            last_prune: SystemTime::now(),
        }
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

        let file_path = tile_cache_path(&self.config.tile_cache_root, coord, scale);

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

        if coord.zoom <= self.config.max_zoom {
            self.delete_indexed_tiles(coord);
            self.delete_parent_tiles(coord);
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
        if coord.zoom <= self.config.index_zoom {
            return;
        }

        let index_path = if let Some(index_coord) = coord.ancestor_at_zoom(self.config.index_zoom) {
            self.index_file_path(index_coord)
        } else {
            return;
        };

        if let Some(parent) = index_path.parent()
            && let Err(err) = fs::create_dir_all(parent)
        {
            eprintln!("create index dir failed: {err}");
            return;
        }

        let mut file = match fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&index_path)
        {
            Ok(file) => file,
            Err(err) => {
                eprintln!("open index file failed: {err}");
                return;
            }
        };

        if let Err(err) =
            file.write_all(format!("{}/{}/{}@{scale}\n", coord.zoom, coord.x, coord.y).as_bytes())
        {
            eprintln!("write index entry failed: {err}");
        }
    }

    fn delete_parent_tiles(&self, coord: TileCoord) {
        if self.config.invalidate_min_zoom > self.config.index_zoom {
            return;
        }

        let mut coord = coord;

        while coord.zoom > self.config.invalidate_min_zoom {
            let Some(parent) = coord.parent() else {
                break;
            };
            coord = parent;

            if coord.zoom > self.config.index_zoom {
                continue;
            }

            self.delete_tile_files(coord);
        }
    }

    fn delete_indexed_tiles(&self, invalidate_coord: TileCoord) {
        if let Some(index_coord) = invalidate_coord.ancestor_at_zoom(self.config.index_zoom) {
            self.process_index_tile(index_coord, invalidate_coord);
        } else {
            let factor = 1 << (self.config.index_zoom - invalidate_coord.zoom);
            let x_start = invalidate_coord.x * factor;
            let y_start = invalidate_coord.y * factor;

            for index_x in x_start..x_start + factor {
                for index_y in y_start..y_start + factor {
                    self.process_index_tile(
                        TileCoord {
                            zoom: self.config.index_zoom,
                            x: index_x,
                            y: index_y,
                        },
                        invalidate_coord,
                    );
                }
            }
        }
    }

    fn process_index_tile(&self, index_coord: TileCoord, target: TileCoord) {
        let index_path = self.index_file_path(index_coord);

        let mut file = match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .open(&index_path)
        {
            Ok(file) => file,
            Err(err) => {
                if err.kind() != ErrorKind::NotFound {
                    eprintln!("failed to open index {}: {err}", index_path.display());
                }
                return;
            }
        };

        let mut contents = String::new();

        if let Err(err) = file.read_to_string(&mut contents) {
            eprintln!("failed to read index {}: {err}", index_path.display());
            return;
        }

        let mut retained = Vec::new();
        let mut removed_any = false;

        for entry in contents.lines() {
            let (coord, scale) = match parse_index_entry(entry) {
                Ok(ok) => ok,
                Err(err) => {
                    eprintln!(
                        "ferror parsing entry {} from {entry}: {err}",
                        index_path.to_string_lossy()
                    );

                    retained.push(entry.to_string());
                    continue;
                }
            };

            if target.is_ancestor_of(coord) {
                removed_any = true;

                let path = tile_cache_path(&self.config.tile_cache_root, coord, scale);

                if let Err(err) = fs::remove_file(&path)
                    && err.kind() != ErrorKind::NotFound
                {
                    eprintln!("failed to remove {}: {err}", path.display());
                }
            } else {
                retained.push(entry.to_string());
            }
        }

        if !removed_any {
            return;
        }

        if let Err(err) = file.set_len(0) {
            eprintln!("failed to truncate index {}: {err}", index_path.display());
            return;
        }

        if retained.is_empty() {
            return;
        }

        let mut rewritten = retained.join("\n");
        rewritten.push('\n');

        if let Err(err) = file.write_all(rewritten.as_bytes()) {
            eprintln!("failed to rewrite index {}: {err}", index_path.display());
        }
    }

    fn delete_tile_files(&self, coord: TileCoord) {
        let dir = self
            .config
            .tile_cache_root
            .join(coord.zoom.to_string())
            .join(coord.x.to_string());

        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) => {
                if err.kind() != ErrorKind::NotFound {
                    eprintln!("failed to read dir {}: {err}", dir.display());
                }
                return;
            }
        };

        let prefix = format!("{}@", coord.y);

        for entry in entries.flatten() {
            let file_name = entry.file_name();

            let file_name = file_name.to_string_lossy();

            if !file_name.starts_with(&prefix) || !file_name.ends_with(".jpeg") {
                continue;
            }

            if let Err(err) = fs::remove_file(entry.path())
                && err.kind() != ErrorKind::NotFound
            {
                eprintln!("failed to remove {}: {err}", entry.path().display());
            }
        }
    }

    fn index_file_path(&self, index_coord: TileCoord) -> PathBuf {
        let mut path = self.config.tile_cache_root.to_path_buf();
        path.push(index_coord.zoom.to_string());
        path.push(index_coord.x.to_string());
        path.push(format!("{}.index", index_coord.y));
        path
    }
}

pub(crate) fn tile_cache_path(base: &std::path::Path, coord: TileCoord, scale: f64) -> PathBuf {
    let mut path = base.to_owned();
    path.push(coord.zoom.to_string());
    path.push(coord.x.to_string());
    path.push(format!("{}@{scale}.jpeg", coord.y));
    path
}

fn parse_index_entry(entry: &str) -> Result<(TileCoord, f64), TileCoordParseError> {
    let (tile_part, scale_part) = entry
        .split_once('@')
        .ok_or(TileCoordParseError::InvalidFormat)?;

    let scale = scale_part
        .parse::<f64>()
        .map_err(TileCoordParseError::ParseFloat)?;

    Ok((tile_part.parse()?, scale))
}
