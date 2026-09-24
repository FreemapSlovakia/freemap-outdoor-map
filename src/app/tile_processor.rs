use crate::{
    app::{tile_coord::TileCoord, tile_processing_worker::SaveTile},
    render::Attribution,
};
use rustix::fs::{XattrFlags, fgetxattr, fsetxattr};
use sled::Batch;
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    os::fd::AsFd,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// Where a cached tile keeps its dataset codes, in the short comma form. On the
/// tile's own inode rather than in `--index`, so the codes go wherever the file
/// goes and are deleted with it.
const ATTRIBUTION_XATTR: &str = "user.attribution";

/// Longest value [`read_attribution`] accepts; nine sources on a triple border
/// take 31 bytes.
const MAX_ATTRIBUTION_LEN: usize = 1024;

#[derive(Clone)]
pub struct VariantConfig {
    pub(crate) tile_cache_base_path: Option<PathBuf>,
    /// Opened by the caller, because the server reads it too — a blank tile is
    /// recorded here instead of written to disk, and serving one is a lookup.
    pub(crate) index_db: Option<sled::Db>,
    /// The variant's format, so invalidation deletes the file the route wrote.
    pub(crate) ext: &'static str,
}

#[derive(Clone)]
pub struct TileProcessingConfig {
    pub(crate) variants: Vec<VariantConfig>,
    pub(crate) invalidate_min_zoom: u8,
}

struct VariantRuntime {
    tile_cache_base_path: Option<PathBuf>,
    db: Option<sled::Db>,
    ext: &'static str,
}

pub struct TileProcessor {
    variants: Vec<VariantRuntime>,
    invalidate_min_zoom: u8,
    invalidation_register: HashMap<TileCoord, SystemTime>,
    last_prune: SystemTime,
}

// Signature is dictated by sled's merge-operator API; the `Option` return
// (None = delete) is part of that contract.
#[allow(clippy::unnecessary_wraps)]
pub(super) fn concatenate_merge(
    _key: &[u8],              // the key being merged
    old_value: Option<&[u8]>, // the previous value, if one existed
    merged_bytes: &[u8],      // the new bytes being merged in
) -> Option<Vec<u8>> {
    // set the new value, return None to delete
    let mut ret = old_value.map(<[u8]>::to_vec).unwrap_or_default();

    ret.extend_from_slice(merged_bytes);

    Some(ret)
}

impl TileProcessor {
    pub(crate) fn new(config: TileProcessingConfig) -> Self {
        let mut variants = Vec::with_capacity(config.variants.len());

        for variant in config.variants {
            variants.push(VariantRuntime {
                tile_cache_base_path: variant.tile_cache_base_path,
                db: variant.index_db,
                ext: variant.ext,
            });
        }

        Self {
            variants,
            invalidate_min_zoom: config.invalidate_min_zoom,
            invalidation_register: HashMap::new(),
            last_prune: SystemTime::now(),
        }
    }

    pub(crate) const fn last_prune(&self) -> SystemTime {
        self.last_prune
    }

    pub(crate) const fn set_last_prune(&mut self, now: SystemTime) {
        self.last_prune = now;
    }

    pub(crate) fn handle_save_tile(&self, tile: SaveTile) {
        let SaveTile {
            data,
            attribution,
            coord,
            scale,
            render_started_at,
            variant_index,
            blank,
        } = tile;

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

        Self::append_index_entry(variant.db.as_ref(), coord, scale, blank);

        // A blank tile is the mark and nothing else: on ext4 a 42-byte file still
        // takes a 4 KiB block and an inode, and a sparse overlay produces these
        // by the million.
        if blank {
            return;
        }

        let file_path = cached_tile_path(tile_cache_base_path, coord, scale, variant.ext);

        if let Some(parent) = file_path.parent()
            && let Err(err) = fs::create_dir_all(parent)
        {
            eprintln!("create tile dir failed: {err}");
        }

        if let Err(err) = write_tile(&file_path, &data, &attribution, render_started_at) {
            eprintln!("write tile {coord}@{scale} failed: {err}");
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

            let ext = variant.ext;

            let mut batch = Batch::default();

            Self::remove_descendants(db, &mut batch, coord, base_path, ext);

            let mut current = coord;
            loop {
                if current.zoom < self.invalidate_min_zoom {
                    break;
                }

                let Some(parent) = current.parent() else {
                    break;
                };

                current = parent;

                Self::remove_exact(db, &mut batch, current, base_path, ext);
            }

            if let Err(err) = db.apply_batch(batch) {
                eprintln!("failed to apply DB remove batch for {coord}: {err}");
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

    fn append_index_entry(db: Option<&sled::Db>, coord: TileCoord, scale: f64, blank: bool) {
        let Some(db) = db else {
            return;
        };

        let key: Vec<u8> = coord.into();

        if let Err(err) = db.merge(key, [index_byte(scale, blank); 1]) {
            eprint!("error merging tile {coord}: {err}");
        }
    }

    fn remove_descendants(
        db: &sled::Db,
        batch: &mut Batch,
        coord: TileCoord,
        base_path: &std::path::Path,
        ext: &str,
    ) {
        let key: Vec<u8> = coord.into();

        for item in db.scan_prefix(key) {
            match item {
                Ok(entry) => {
                    let entry_coord = entry.0.as_ref().into();
                    Self::remove_files(entry_coord, entry.1.as_ref(), base_path, ext);
                    batch.remove(entry.0);
                }
                Err(err) => {
                    eprintln!("error scanning {coord}: {err}");
                }
            }
        }
    }

    fn remove_exact(
        db: &sled::Db,
        batch: &mut Batch,
        coord: TileCoord,
        base_path: &std::path::Path,
        ext: &str,
    ) {
        let key: Vec<u8> = coord.into();

        let scales = match db.get(key.clone()) {
            Ok(Some(scales)) => scales,
            Ok(None) => return,
            Err(err) => {
                eprintln!("failed to get {coord} from DB: {err}");
                return;
            }
        };

        Self::remove_files(coord, scales.as_ref(), base_path, ext);
        batch.remove(key);
    }

    fn remove_files(coord: TileCoord, scales: &[u8], base_path: &std::path::Path, ext: &str) {
        let unique_scales: HashSet<u8> = scales.iter().copied().collect();

        for scale in unique_scales {
            // A blank scale has no file; unlinking a missing one is already
            // tolerated below, so the mark only has to be taken off the scale.
            let path = cached_tile_path(base_path, coord, f64::from(scale & !BLANK_MARK), ext);

            if let Err(err) = fs::remove_file(&path)
                && err.kind() != io::ErrorKind::NotFound
            {
                eprintln!("failed to remove file {}: {err}", path.display());
            }
        }
    }
}

/// Writes the tile beside its final path and renames it into place, so a reader
/// sees either the old tile or the new one whole — never new bytes with the old
/// codes, or a partial file.
fn write_tile(
    path: &Path,
    data: &[u8],
    attribution: &Attribution,
    modified: SystemTime,
) -> io::Result<()> {
    // The processing worker is the only writer, so a fixed name cannot collide.
    let tmp_path = path.with_extension("jpeg.tmp");

    let result = (|| {
        let mut file = fs::File::create(&tmp_path)?;

        // Unknown codes only widen the credit, so a filesystem without user xattrs
        // still caches the tile.
        if let Err(err) = write_attribution(&file, attribution) {
            eprintln!("set tile attribution failed: {err}");
        }

        io::Write::write_all(&mut file, data)?;

        file.set_times(fs::FileTimes::new().set_modified(modified))?;

        fs::rename(&tmp_path, path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }

    result
}

fn write_attribution(file: &fs::File, attribution: &Attribution) -> io::Result<()> {
    fsetxattr(
        file,
        ATTRIBUTION_XATTR,
        attribution.encode().as_bytes(),
        XattrFlags::empty(),
    )
    .map_err(io::Error::from)
}

/// The codes a cached tile was saved with, or `None` when the filesystem could not
/// store them.
pub fn read_attribution(file: impl AsFd) -> Option<Attribution> {
    let mut value = [0u8; MAX_ATTRIBUTION_LEN];

    let len = fgetxattr(file, ATTRIBUTION_XATTR, &mut value).ok()?;

    std::str::from_utf8(&value[..len])
        .ok()
        .map(Attribution::decode)
}

/// Set on a scale byte in the index to say the tile was blank, so no file was
/// written. Scales are 1..=3, so the top bit is free.
pub const BLANK_MARK: u8 = 0x80;

/// The byte a tile contributes to its index entry.
pub const fn index_byte(scale: f64, blank: bool) -> u8 {
    let scale = scale as u8;

    if blank { scale | BLANK_MARK } else { scale }
}

pub fn cached_tile_path(
    base: &std::path::Path,
    coord: TileCoord,
    scale: f64,
    ext: &str,
) -> PathBuf {
    let mut path = base.to_owned();
    path.push(coord.zoom.to_string());
    path.push(coord.x.to_string());
    path.push(format!("{}@{scale}.{ext}", coord.y));
    path
}

#[cfg(test)]
mod tests {
    use super::{read_attribution, write_tile};
    use crate::render::Attribution;
    use std::{fs, time::SystemTime};

    /// Removes the tile even when an assertion fails.
    struct TempTile(std::path::PathBuf);

    impl Drop for TempTile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn a_saved_tile_keeps_known_empty_apart_from_unknown() {
        let tile =
            TempTile(std::env::temp_dir().join(format!("tile-xattr-{}.jpeg", std::process::id())));

        fs::write(&tile.0, b"old").expect("tile written");

        // Where user xattrs are unsupported there is nothing to test.
        if let Err(err) = rustix::fs::fgetxattr(
            fs::File::open(&tile.0).expect("tile opened"),
            "user.attribution",
            &mut [0u8; 1],
        ) && err == rustix::io::Errno::NOTSUP
        {
            return;
        }

        let read = || read_attribution(fs::File::open(&tile.0).expect("tile opened"));

        assert_eq!(read(), None);

        let mut attribution = Attribution::default();
        attribution.add_osm();
        attribution.add_shading("sk");

        write_tile(&tile.0, b"new", &attribution, SystemTime::now()).expect("tile saved");
        assert_eq!(read(), Some(attribution));
        assert_eq!(fs::read(&tile.0).expect("tile read"), b"new");

        // A re-render crediting nothing replaces the codes rather than keeping them.
        write_tile(&tile.0, b"gray", &Attribution::default(), SystemTime::now())
            .expect("tile saved");
        assert_eq!(read(), Some(Attribution::default()));
    }
}
