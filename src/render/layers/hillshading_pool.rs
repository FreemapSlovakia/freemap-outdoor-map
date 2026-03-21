use gdal::Dataset;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::JoinHandle,
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

const EVICT_AFTER: Duration = Duration::from_secs(60);
const EVICT_INTERVAL: Duration = Duration::from_secs(10);

/// Raw pixel data returned from a dataset actor, ready to be wrapped in an ImageSurface.
pub struct RawSurface {
    pub data: Vec<u8>,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
}

/// Parameters for a dataset read request.
pub struct ReadRequest {
    pub bbox: geo::Rect<f64>,
    pub size: crate::render::size::Size<u32>,
    pub scale: f64,
    pub mode: super::hillshading::Mode,
    pub resp_tx: mpsc::Sender<Result<Option<RawSurface>, gdal::errors::GdalError>>,
}

struct Actor {
    tx: SyncSender<ReadRequest>,
    handle: Option<JoinHandle<()>>,
    last_used: Instant,
    busy: bool,
}

struct CountryActors {
    actors: Vec<Actor>,
}

pub struct HillshadingPool {
    base: PathBuf,
    state: Mutex<HashMap<String, CountryActors>>,
    shutdown: AtomicBool,
}

impl HillshadingPool {
    pub fn new(base: impl AsRef<Path>) -> Self {
        Self {
            base: base.as_ref().to_path_buf(),
            state: Mutex::new(HashMap::new()),
            shutdown: AtomicBool::new(false),
        }
    }

    /// Read hillshading data for a country. Blocks until a result is available.
    /// Returns None if the tile has no data for this country, or if the country is unknown.
    pub fn read(
        &self,
        country: &str,
        bbox: geo::Rect<f64>,
        size: crate::render::size::Size<u32>,
        scale: f64,
        mode: super::hillshading::Mode,
    ) -> Result<Option<RawSurface>, gdal::errors::GdalError> {
        let (resp_tx, resp_rx) = mpsc::channel();

        let request = ReadRequest {
            bbox,
            size,
            scale,
            mode,
            resp_tx,
        };

        {
            let mut state = self.state.lock().unwrap();
            let country_actors = state
                .entry(country.to_string())
                .or_insert_with(|| CountryActors { actors: Vec::new() });

            // Try to find an idle actor.
            if let Some(actor) = country_actors.actors.iter_mut().find(|a| !a.busy) {
                actor.busy = true;
                actor.last_used = Instant::now();

                if actor.tx.send(request).is_ok() {
                    drop(state);
                    let result = recv_result(resp_rx);
                    self.mark_idle(country);
                    return result;
                }

                // Actor thread is dead — remove it and try spawning.
                return Err(gdal::errors::GdalError::NullPointer {
                    method_name: "pool send",
                    msg: "actor thread dead".into(),
                });
            }

            // All busy or none exist — spawn a new actor.
            let Some(path) = dataset_path(country) else {
                eprintln!("Unknown hillshading dataset key: {country}");
                return Ok(None);
            };

            let full_path = self.base.join(path);
            let spawned = spawn_actor(full_path);

            let req = request;
            let send_ok = spawned.tx.send(req).is_ok();

            country_actors.actors.push(Actor {
                tx: spawned.tx,
                handle: Some(spawned.handle),
                last_used: Instant::now(),
                busy: true,
            });

            if send_ok {
                drop(state);
                let result = recv_result(resp_rx);
                self.mark_idle(country);
                return result;
            }

            Err(gdal::errors::GdalError::NullPointer {
                method_name: "pool send",
                msg: "newly spawned actor dead".into(),
            })
        }
    }

    /// Mark an actor as no longer busy.
    fn mark_idle(&self, country: &str) {
        let mut state = self.state.lock().unwrap();
        if let Some(country_actors) = state.get_mut(country) {
            // Mark the first busy actor as idle. This is approximate but sufficient since
            // we only need to track the count of busy actors, not which specific one.
            if let Some(actor) = country_actors.actors.iter_mut().find(|a| a.busy) {
                actor.busy = false;
                actor.last_used = Instant::now();
            }
        }
    }

    /// Evict actors that have been idle longer than EVICT_AFTER.
    pub fn evict_unused(&self) {
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();

        for (_, country_actors) in state.iter_mut() {
            country_actors.actors.retain_mut(|actor| {
                if actor.busy {
                    return true;
                }

                if now.duration_since(actor.last_used) <= EVICT_AFTER {
                    return true;
                }

                // Drop the sender — actor thread will exit.
                // Take the handle so we can join.
                if let Some(handle) = actor.handle.take() {
                    // Join in a detached manner — don't block the evictor.
                    std::thread::Builder::new()
                        .name("dataset-actor-joiner".into())
                        .spawn(move || {
                            let _ = handle.join();
                        })
                        .ok();
                }

                false
            });
        }

        // Remove empty country entries.
        state.retain(|_, ca| !ca.actors.is_empty());
    }

    /// Start background eviction thread.
    pub fn start_evictor(self: &Arc<Self>) -> JoinHandle<()> {
        let pool = self.clone();

        std::thread::Builder::new()
            .name("hillshading-evictor".into())
            .spawn(move || {
                while !pool.shutdown.load(Ordering::Relaxed) {
                    std::thread::park_timeout(EVICT_INTERVAL);
                    if !pool.shutdown.load(Ordering::Relaxed) {
                        pool.evict_unused();
                    }
                }
            })
            .expect("evictor thread spawn")
    }

    pub fn shutdown(&self, evictor: Option<JoinHandle<()>>) {
        self.shutdown.store(true, Ordering::Relaxed);

        if let Some(handle) = &evictor {
            handle.thread().unpark();
        }

        let mut state = self.state.lock().unwrap();

        for (_, country_actors) in state.iter_mut() {
            for actor in country_actors.actors.drain(..) {
                drop(actor.tx);

                if let Some(handle) = actor.handle {
                    let _ = handle.join();
                }
            }
        }

        state.clear();

        if let Some(handle) = evictor {
            let _ = handle.join();
        }
    }
}

fn recv_result(
    rx: mpsc::Receiver<Result<Option<RawSurface>, gdal::errors::GdalError>>,
) -> Result<Option<RawSurface>, gdal::errors::GdalError> {
    rx.recv()
        .unwrap_or(Err(gdal::errors::GdalError::NullPointer {
            method_name: "pool recv",
            msg: "actor dropped".into(),
        }))
}

struct SpawnedActor {
    tx: SyncSender<ReadRequest>,
    handle: JoinHandle<()>,
}

fn spawn_actor(path: PathBuf) -> SpawnedActor {
    let (tx, rx) = mpsc::sync_channel::<ReadRequest>(0);

    let handle = std::thread::Builder::new()
        .name(format!("dataset-{}", path.display()))
        .spawn(move || {
            let dataset = match Dataset::open(&path) {
                Ok(ds) => ds,
                Err(err) => {
                    eprintln!(
                        "Error opening hillshading geotiff {}: {}",
                        path.display(),
                        err
                    );
                    // Drain remaining requests with errors.
                    for req in rx.iter() {
                        let _ = req.resp_tx.send(Err(gdal::errors::GdalError::NullPointer {
                            method_name: "Dataset::open",
                            msg: format!("failed to open {}", path.display()),
                        }));
                    }
                    return;
                }
            };

            for req in rx.iter() {
                let result = read_rgba_from_gdal(&dataset, req.bbox, req.size, req.scale, req.mode);
                let _ = req.resp_tx.send(result);
            }
        })
        .expect("dataset actor spawn");

    SpawnedActor { tx, handle }
}

fn dataset_path(name: &str) -> Option<&'static str> {
    DATASET_PATHS
        .iter()
        .find(|(dataset_name, _)| dataset_name == &name)
        .map(|(_, path)| *path)
}

/// GDAL read logic extracted from hillshading.rs, returns raw pixel data instead of ImageSurface.
fn read_rgba_from_gdal(
    dataset: &Dataset,
    bbox: geo::Rect<f64>,
    size: crate::render::size::Size<u32>,
    scale: f64,
    mode: super::hillshading::Mode,
) -> Result<Option<RawSurface>, gdal::errors::GdalError> {
    let min = bbox.min();
    let max = bbox.max();

    let [gt_x_off, gt_x_width, _, gt_y_off, _, gt_y_width] = dataset.geo_transform()?;

    let pixel_min_x_f = (min.x - gt_x_off) / gt_x_width;
    let pixel_max_x_f = (max.x - gt_x_off) / gt_x_width;

    let pixel_min_x = pixel_min_x_f.floor() as isize;
    let pixel_max_x = pixel_max_x_f.ceil() as isize;

    let (pixel_min_y_f, pixel_max_y_f) = {
        let pixel_y0 = (min.y - gt_y_off) / gt_y_width;
        let pixel_y1 = (max.y - gt_y_off) / gt_y_width;

        (pixel_y0.min(pixel_y1), pixel_y0.max(pixel_y1))
    };

    let pixel_min_y = pixel_min_y_f.floor() as isize;
    let pixel_max_y = pixel_max_y_f.ceil() as isize;

    let window_width_px = (pixel_max_x - pixel_min_x) as usize;
    let window_height_px = (pixel_max_y - pixel_min_y) as usize;

    let scaled_width_px = (size.width as f64 * scale) as usize;
    let scaled_height_px = (size.height as f64 * scale) as usize;

    let scale_x = scaled_width_px as f64 / (pixel_max_x_f - pixel_min_x_f).abs().max(1e-6);
    let scale_y = scaled_height_px as f64 / (pixel_max_y_f - pixel_min_y_f).abs().max(1e-6);

    let buffered_w = (scale_x * window_width_px as f64).ceil().max(1.0) as usize;
    let buffered_h = (scale_y * window_height_px as f64).ceil().max(1.0) as usize;

    let mut rgba_data = vec![0u8; buffered_w * buffered_h * 4];

    let (raster_width, raster_height) = dataset.raster_size();

    let clamped_window_x = pixel_min_x.max(0).min(raster_width as isize);
    let clamped_window_y = pixel_min_y.max(0).min(raster_height as isize);

    let clamped_source_width = ((pixel_min_x + window_width_px as isize).min(raster_width as isize)
        - clamped_window_x)
        .max(0) as usize;

    let clamped_source_height =
        ((pixel_min_y + window_height_px as isize).min(raster_height as isize) - clamped_window_y)
            .max(0) as usize;

    if clamped_source_width == 0 || clamped_source_height == 0 {
        return Ok(None);
    }

    let resampled_width = (buffered_w as f64
        * (clamped_source_width as f64 / window_width_px as f64))
        .ceil() as usize;

    let resampled_height = (buffered_h as f64
        * (clamped_source_height as f64 / window_height_px as f64))
        .ceil() as usize;

    let offset_x = (((clamped_window_x - pixel_min_x) as f64 / window_width_px as f64)
        * buffered_w as f64)
        .floor()
        .max(0.0) as usize;

    let offset_y = (((clamped_window_y - pixel_min_y) as f64 / window_height_px as f64)
        * buffered_h as f64)
        .floor()
        .max(0.0) as usize;

    let copy_width = resampled_width.min(buffered_w.saturating_sub(offset_x));
    let copy_height = resampled_height.min(buffered_h.saturating_sub(offset_y));

    let mut band_buffer = vec![0u8; resampled_height * resampled_width];

    if dataset.raster_count() != 4 {
        panic!("unsupported band count");
    }

    if matches!(mode, super::hillshading::Mode::Shading) {
        for band_index in 0..3 {
            let band = dataset.rasterband(band_index + 1)?;

            if clamped_source_width > 0
                && clamped_source_height > 0
                && resampled_width > 0
                && resampled_height > 0
            {
                band.read_into_slice::<u8>(
                    (clamped_window_x, clamped_window_y),
                    (clamped_source_width, clamped_source_height),
                    (resampled_width, resampled_height),
                    &mut band_buffer,
                    Some(gdal::raster::ResampleAlg::Lanczos),
                )?;
            }

            for y in 0..copy_height {
                for x in 0..copy_width {
                    let data_index = y * resampled_width + x;
                    let rgba_index = ((y + offset_y) * buffered_w + (x + offset_x)) * 4;
                    rgba_data[rgba_index + band_index] = band_buffer[data_index];
                }
            }
        }
    }

    let alpha_band = dataset.rasterband(4)?;

    let alpha_no_data = alpha_band.no_data_value().map(|nd| nd as u8);

    let mask_band = alpha_band
        .mask_flags()
        .ok()
        .filter(|f| f.is_per_dataset())
        .and_then(|_| alpha_band.open_mask_band().ok());

    let mut mask_buffer = if mask_band.is_some() {
        Some(vec![0u8; resampled_height * resampled_width])
    } else {
        None
    };

    if clamped_source_width > 0
        && clamped_source_height > 0
        && resampled_width > 0
        && resampled_height > 0
    {
        alpha_band.read_into_slice::<u8>(
            (clamped_window_x, clamped_window_y),
            (clamped_source_width, clamped_source_height),
            (resampled_width, resampled_height),
            &mut band_buffer,
            Some(gdal::raster::ResampleAlg::Lanczos),
        )?;

        if let (Some(mask_band), Some(mask_buffer)) = (mask_band.as_ref(), mask_buffer.as_mut()) {
            mask_band.read_into_slice::<u8>(
                (clamped_window_x, clamped_window_y),
                (clamped_source_width, clamped_source_height),
                (resampled_width, resampled_height),
                mask_buffer,
                Some(gdal::raster::ResampleAlg::Lanczos),
            )?;
        }
    }

    let mut has_data = false;

    for y in 0..copy_height {
        for x in 0..copy_width {
            let (alpha, mask_alpha) = {
                let data_index = y * resampled_width + x;

                let value = band_buffer[data_index];

                if alpha_no_data.is_some_and(|nd| nd == value)
                    || mask_buffer
                        .as_ref()
                        .is_some_and(|mask_buffer| mask_buffer[data_index] == 0)
                {
                    (0, 0)
                } else {
                    has_data = true;

                    (value, 255)
                }
            };

            let rgba_index = ((y + offset_y) * buffered_w + (x + offset_x)) * 4;

            match mode {
                super::hillshading::Mode::Shading => {
                    rgba_data[rgba_index + 3] = alpha;
                }
                super::hillshading::Mode::Mask => {
                    rgba_data[rgba_index] = 255;
                    rgba_data[rgba_index + 1] = 255;
                    rgba_data[rgba_index + 2] = 255;
                    rgba_data[rgba_index + 3] = mask_alpha;
                }
            }
        }
    }

    let (crop_x, crop_y) = {
        let frac_x = pixel_min_x_f - pixel_min_x as f64;
        let frac_y = pixel_min_y_f - pixel_min_y as f64;

        let crop_x_base = offset_x + (frac_x * scale_x).round().max(0.0) as usize;
        let crop_y_base = offset_y + (frac_y * scale_y).round().max(0.0) as usize;

        let crop_x = crop_x_base.min(buffered_w.saturating_sub(scaled_width_px));
        let crop_y = crop_y_base.min(buffered_h.saturating_sub(scaled_height_px));

        (crop_x, crop_y)
    };

    let crop_w = scaled_width_px.min(buffered_w.saturating_sub(crop_x));
    let crop_h = scaled_height_px.min(buffered_h.saturating_sub(crop_y));

    let mut final_rgba_data = vec![0u8; scaled_width_px * scaled_height_px * 4];

    if crop_w > 0 && crop_h > 0 && crop_x < buffered_w && crop_y < buffered_h {
        for y in 0..crop_h {
            let src_offset = ((y + crop_y) * buffered_w + crop_x) * 4;
            let dst_offset = y * scaled_width_px * 4;

            let max_copy = ((buffered_w - crop_x) * 4).min(crop_w * 4);
            let src_end = (src_offset + max_copy).min(rgba_data.len());
            let dst_end = dst_offset + (src_end - src_offset);

            if src_end > src_offset && dst_end > dst_offset {
                final_rgba_data[dst_offset..dst_end]
                    .copy_from_slice(&rgba_data[src_offset..src_end]);
            }
        }
    }

    for i in (0..final_rgba_data.len()).step_by(4) {
        let alpha = final_rgba_data[i + 3] as f32 / 255.0;

        let r = (final_rgba_data[i] as f32 * alpha) as u8;
        let g = (final_rgba_data[i + 1] as f32 * alpha) as u8;
        let b = (final_rgba_data[i + 2] as f32 * alpha) as u8;

        final_rgba_data[i] = b;
        final_rgba_data[i + 1] = g;
        final_rgba_data[i + 2] = r;
    }

    if !has_data {
        return Ok(None);
    }

    let width = (size.width as f64 * scale) as i32;
    let height = (size.height as f64 * scale) as i32;
    let stride = width * 4;

    Ok(Some(RawSurface {
        data: final_rgba_data,
        width,
        height,
        stride,
    }))
}
