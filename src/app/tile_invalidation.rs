use crate::app::index_paths::index_file_path;
use fs2::FileExt;
use notify::{EventKind, RecursiveMode, Watcher};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

pub(crate) struct InvalidationConfig {
    pub(crate) watch_base: PathBuf,
    pub(crate) tile_base_path: PathBuf,
    pub(crate) parent_min_zoom: u32,
    pub(crate) index_zoom: u32,
    pub(crate) max_zoom: u32,
}

pub(crate) fn process_recovery_files(config: &InvalidationConfig) {
    let mut pending = Vec::new();

    let base = config.tile_base_path.join(config.index_zoom.to_string());

    collect_processing_files(&base, &mut pending);

    for path in pending {
        if let Err(err) = process_processing_file(config, &path) {
            eprintln!("failed to process recovery {}: {err}", path.display());
        }
    }
}

pub(crate) fn process_existing_expiration_files(config: &InvalidationConfig) {
    let mut pending = Vec::new();

    collect_expiration_files(&config.watch_base, &mut pending);

    for path in pending {
        if let Err(err) = process_tile_expiration_file(config, &path) {
            eprintln!(
                "tile expiration processing failed for {}: {err}",
                path.display()
            );
        }
    }
}

pub(crate) fn start_watcher(config: InvalidationConfig) {
    thread::Builder::new()
        .name("imposm-tile-watcher".to_string())
        .spawn(move || run_watcher(config))
        .expect("spawn imposm watcher");
}

fn run_watcher(config: InvalidationConfig) {
    let (tx, rx) = mpsc::channel();

    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    }) {
        Ok(watcher) => watcher,
        Err(err) => {
            eprintln!("imposm watcher init failed: {err}");
            return;
        }
    };

    if let Err(err) = watcher.watch(&config.watch_base, RecursiveMode::Recursive) {
        eprintln!(
            "imposm watcher failed to watch {}: {err}",
            config.watch_base.display()
        );

        return;
    }

    for res in rx {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                eprintln!("imposm watcher error: {err}");
                continue;
            }
        };

        if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
            continue;
        }

        for path in event.paths {
            if path.extension().and_then(|ext| ext.to_str()) != Some("tiles") {
                continue;
            }

            println!("Processing {}", path.display());

            if let Err(err) = process_tile_expiration_file(&config, &path) {
                eprintln!(
                    "tile expiration processing failed for {}: {err}",
                    path.display()
                );
            }
        }
    }
}

fn process_tile_expiration_file(config: &InvalidationConfig, path: &Path) -> Result<(), String> {
    let content = match read_with_retry(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.to_string()),
    };

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        if let Some((zoom, x, y)) = parse_tile_line(line) {
            invalidate_tile_pyramid(config, zoom, x, y);
        } else {
            eprintln!("invalid tile line: {line}");
        }
    }

    if let Err(err) = fs::remove_file(path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("failed to remove tile file {}: {err}", path.display());
    }

    Ok(())
}

fn read_with_retry(path: &Path) -> std::io::Result<String> {
    let mut last_err = None;
    for _ in 0..5 {
        let size_before = match fs::metadata(path) {
            Ok(meta) => meta.len(),
            Err(err) => {
                last_err = Some(err);
                thread::sleep(Duration::from_millis(50));
                continue;
            }
        };

        match fs::read_to_string(path) {
            Ok(value) => {
                let size_after = fs::metadata(path)
                    .map(|meta| meta.len())
                    .unwrap_or(size_before);
                let stable = size_before == size_after;
                let complete = value.is_empty() || value.ends_with('\n');
                if stable && complete {
                    return Ok(value);
                }
                last_err = Some(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "file still changing",
                ));
            }
            Err(err) => {
                last_err = Some(err);
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("read failed")))
}

fn parse_tile_line(line: &str) -> Option<(u32, u32, u32)> {
    let mut iter = line.split('/');
    let zoom = iter.next()?.parse::<u32>().ok()?;
    let x = iter.next()?.parse::<u32>().ok()?;
    let y = iter.next()?.parse::<u32>().ok()?;
    if iter.next().is_some() {
        return None;
    }
    Some((zoom, x, y))
}

fn invalidate_tile_pyramid(config: &InvalidationConfig, zoom: u32, x: u32, y: u32) {
    if zoom <= config.max_zoom {
        delete_tile_files(&config.tile_base_path, zoom, x, y);
    }
    delete_parent_tiles(config, zoom, x, y);
    if zoom <= config.max_zoom {
        delete_indexed_children(config, zoom, x, y);
    }
}

fn delete_parent_tiles(config: &InvalidationConfig, zoom: u32, x: u32, y: u32) {
    let mut zoom = zoom;
    let mut x = x;
    let mut y = y;

    while zoom > config.parent_min_zoom {
        zoom -= 1;
        x /= 2;
        y /= 2;
        delete_tile_files(&config.tile_base_path, zoom, x, y);
    }
}

fn delete_indexed_children(config: &InvalidationConfig, zoom: u32, x: u32, y: u32) {
    if zoom != config.index_zoom {
        eprintln!(
            "skipping indexed child deletion for {zoom}/{x}/{y}: expected index zoom {}",
            config.index_zoom
        );

        return;
    }

    let index_path = index_file_path(&config.tile_base_path, config.index_zoom, x, y);

    let processing_path = match snapshot_to_processing(&index_path) {
        Ok(Some(path)) => path,
        Ok(None) => return,
        Err(err) => {
            eprintln!("failed to snapshot index {}: {err}", index_path.display());
            return;
        }
    };

    if let Err(err) = process_processing_file(config, &processing_path) {
        eprintln!("failed to process {}: {err}", processing_path.display());
    }
}

fn delete_tile_files(base: &Path, zoom: u32, x: u32, y: u32) {
    let dir = base.join(zoom.to_string()).join(x.to_string());

    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                eprintln!("failed to read dir {}: {err}", dir.display());
            }
            return;
        }
    };

    let prefix = format!("{y}@");

    for entry in entries.flatten() {
        let file_name = entry.file_name();

        let file_name = file_name.to_string_lossy();

        if !file_name.starts_with(&prefix) || !file_name.ends_with(".jpeg") {
            continue;
        }

        if let Err(err) = fs::remove_file(entry.path())
            && err.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("failed to remove {}: {err}", entry.path().display());
        }
    }
}

fn snapshot_to_processing(path: &Path) -> std::io::Result<Option<PathBuf>> {
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(false)
        .open(path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };

    file.lock_exclusive()?;

    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let processing_path = processing_path(path);
    let processing_path = unique_processing_path(&processing_path);

    let mut processing_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&processing_path)?;

    processing_file.write_all(contents.as_bytes())?;

    drop(processing_file);

    file.set_len(0)?;

    Ok(Some(processing_path))
}

fn process_processing_file(config: &InvalidationConfig, path: &Path) -> std::io::Result<()> {
    let mut contents = String::new();

    let mut file = std::fs::File::open(path)?;

    file.read_to_string(&mut contents)?;

    for entry in contents.lines() {
        let entry = entry.trim();

        if entry.is_empty() {
            continue;
        }

        let path = config.tile_base_path.join(format!("{entry}.jpeg"));

        if let Err(err) = fs::remove_file(&path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("failed to remove {}: {err}", path.display());
        }
    }

    fs::remove_file(path)?;

    Ok(())
}

fn collect_processing_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                eprintln!("failed to read dir {}: {err}", dir.display());
            }
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            collect_processing_files(&path, out);
            continue;
        }

        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.contains(".index.processing")
        {
            out.push(path);
        }
    }
}

fn collect_expiration_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                eprintln!("failed to read dir {}: {err}", dir.display());
            }
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            collect_expiration_files(&path, out);
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) == Some("tiles") {
            out.push(path);
        }
    }
}

fn processing_path(index_path: &Path) -> PathBuf {
    let mut path = index_path.to_path_buf();

    let file_name = index_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("index");

    let new_name = format!("{file_name}.processing");

    path.set_file_name(new_name);

    path
}

fn unique_processing_path(base: &Path) -> PathBuf {
    if !base.exists() {
        return base.to_path_buf();
    }

    let mut counter = 1;

    loop {
        let candidate = base.with_extension(format!("processing.{counter}"));

        if !candidate.exists() {
            return candidate;
        }

        counter += 1;
    }
}
