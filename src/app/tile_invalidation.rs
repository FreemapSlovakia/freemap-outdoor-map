use crate::app::tile_processing_worker::TileProcessingWorker;
use notify::{EventKind, RecursiveMode, Watcher};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime},
};

pub(crate) fn process_existing_expiration_files(watch_base: &Path, worker: &TileProcessingWorker) {
    let mut pending = Vec::new();

    collect_expiration_files(watch_base, &mut pending);

    for path in pending {
        if let Err(err) = process_tile_expiration_file(path.as_path(), worker) {
            eprintln!(
                "tile expiration processing failed for {}: {err}",
                path.display()
            );
        }
    }
}

pub(crate) fn start_watcher(watch_base: &Path, worker: TileProcessingWorker) {
    let watch_base = watch_base.to_owned();

    thread::Builder::new()
        .name("expired-tiles-watcher".to_string())
        .spawn(move || run_watcher(watch_base.as_path(), worker))
        .expect("spawn expired tiles watcher");
}

fn run_watcher(watch_base: &Path, worker: TileProcessingWorker) {
    let (tx, rx) = mpsc::channel();

    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    }) {
        Ok(watcher) => watcher,
        Err(err) => {
            eprintln!("expired tiles watcher init failed: {err}");
            return;
        }
    };

    if let Err(err) = watcher.watch(watch_base, RecursiveMode::Recursive) {
        eprintln!(
            "expired tiles watcher failed to watch {}: {err}",
            watch_base.display()
        );

        return;
    }

    for res in rx {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                eprintln!("expired tiles watcher error: {err}");
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

            if let Err(err) = process_tile_expiration_file(&path, &worker) {
                eprintln!(
                    "tile expiration processing failed for {}: {err}",
                    path.display()
                );
            }
        }
    }
}

fn process_tile_expiration_file(path: &Path, worker: &TileProcessingWorker) -> Result<(), String> {
    let content = match read_with_retry(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return if err.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(err.to_string())
            };
        }
    };

    println!("Processing {}", path.display());

    let invalidated_at = SystemTime::now();

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        if let Ok(coord) = line.parse() {
            if let Err(err) = worker.invalidate_blocking(coord, invalidated_at) {
                eprintln!("failed to enqueue invalidation for {coord}: {err}");
            }
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
