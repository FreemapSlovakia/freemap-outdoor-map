use gdal::{Dataset, raster::RasterBand};
use geo::{Coord, Rect};

/// Edge of the footprint grid. ~8 KB of bits per dataset.
const GRID: usize = 256;

/// A cell summarises at least this many source pixels per axis, when a level that fine
/// fits the budget below. Coarse overviews have already lost thin features, and the
/// loss roughly doubles per level: measured against its level 3, France drops 130 of
/// its 33772 data cells read at level 7, 82 at level 6, 40 at level 5.
const SOURCE_PER_CELL: usize = 16;

/// Cells of margin added around the data, which takes those leftovers to zero: France
/// keeps 1 miss at its level undilated, none at this radius. Costs ~4 points of
/// acceptance, against 50-60% of reads saved.
const DILATION: usize = 2;

/// The source level is read whole, so this caps both the read and the memory.
const MAX_SOURCE_PIXELS: usize = 32 << 20;

/// The bbox in a dataset's pixel coordinates, before clamping to the raster. Shared
/// by the footprint test and the read itself so the two mappings cannot drift apart.
pub struct PixelWindow {
    pub min: Coord<f64>,
    pub max: Coord<f64>,
}

impl PixelWindow {
    pub fn new(geo_transform: &[f64; 6], bbox: &Rect<f64>) -> Self {
        let [gt_x_off, gt_x_width, _, gt_y_off, _, gt_y_width] = *geo_transform;

        let min = bbox.min();
        let max = bbox.max();

        let x0 = (min.x - gt_x_off) / gt_x_width;
        let x1 = (max.x - gt_x_off) / gt_x_width;

        // The y axis of a north-up transform runs the other way.
        let y0 = (min.y - gt_y_off) / gt_y_width;
        let y1 = (max.y - gt_y_off) / gt_y_width;

        Self {
            min: Coord {
                x: x0.min(x1),
                y: y0.min(y1),
            },
            max: Coord {
                x: x0.max(x1),
                y: y0.max(y1),
            },
        }
    }

    pub const fn min_x(&self) -> isize {
        self.min.x.floor() as isize
    }

    pub const fn max_x(&self) -> isize {
        self.max.x.ceil() as isize
    }

    pub const fn min_y(&self) -> isize {
        self.min.y.floor() as isize
    }

    pub const fn max_y(&self) -> isize {
        self.max.y.ceil() as isize
    }
}

/// A coarse bitset of where a dataset has data, built once from its mask band.
///
/// Only the raster bbox is known up front, and for most countries that bbox is mostly
/// empty — the Dutch data touches all four edges of its own extent while filling under
/// half of it. The footprint answers "is there anything under this tile" without
/// checking a handle out of the pool.
///
/// It only ever over-accepts: a wrongly accepted cell costs one wasted read, a wrongly
/// rejected one silently drops shading from the map.
pub struct Footprint {
    geo_transform: [f64; 6],
    raster_width: usize,
    raster_height: usize,
    grid_width: usize,
    grid_height: usize,
    words_per_row: usize,
    bits: Vec<u64>,
}

/// Unwraps a GDAL result, saying which step gave up on the footprint. Silence here
/// would be indistinguishable from a working one: the dataset keeps paying the full
/// checkout and read, and only the missing speed-up would ever hint at it.
fn attempt<T>(name: &str, step: &str, result: gdal::errors::Result<T>) -> Option<T> {
    result
        .map_err(|err| eprintln!("hillshading {name}: no footprint, {step} failed: {err}"))
        .ok()
}

impl Footprint {
    /// `None` means "never skip this dataset": nothing to tell data from nodata, no
    /// level small enough to read whole, or a failed read.
    pub fn build(dataset: &Dataset, name: &str) -> Option<Self> {
        let alpha = attempt(name, "opening band 4", dataset.rasterband(4))?;

        let mask = match alpha.mask_flags() {
            Ok(flags) if flags.is_per_dataset() => alpha.open_mask_band().ok(),
            _ => None,
        };

        // The same two tests `read_rgba_from_gdal` uses to decide a pixel holds nothing:
        // a per-dataset mask reads 0, or the alpha band reads its nodata value. Either
        // alone over-accepts, which is the safe direction. The `_` fallback has neither
        // — it is ALL_VALID and covers everything, so it needs no footprint.
        let (band, nodata) = match (mask.as_ref(), alpha.no_data_value()) {
            (Some(mask), _) => (mask, 0),
            (None, Some(value)) => (&alpha, value as u8),
            (None, None) => return None,
        };

        let (raster_width, raster_height) = dataset.raster_size();

        if raster_width == 0 || raster_height == 0 {
            return None;
        }

        let Some((level, source)) = pick_source(band) else {
            eprintln!(
                "hillshading {name}: no level under {MAX_SOURCE_PIXELS} pixels, no footprint"
            );

            return None;
        };

        let grid_width = GRID.min(source.0);
        let grid_height = GRID.min(source.1);

        let overview = attempt(
            name,
            "opening the overview",
            level.map(|level| band.overview(level)).transpose(),
        )?;
        let source_band = overview.as_ref().unwrap_or(band);

        let mut pixels = vec![0u8; source.0 * source.1];

        // Read whole and reduce with an OR below, rather than letting GDAL resample.
        // Nearest point-samples, dropping thin features — estuaries, river channels,
        // the Wadden islands. Average survives those but rounds a lone valid pixel to
        // zero once a cell covers more than 510 of them (255/N < 0.5), which costs
        // Sweden 22 cells at this level. An OR cannot lose a pixel either way.
        if let Err(err) =
            source_band.read_into_slice::<u8>((0, 0), source, source, &mut pixels, None)
        {
            eprintln!("hillshading {name}: footprint read failed, never skipping: {err}");

            return None;
        }

        let words_per_row = grid_width.div_ceil(64);

        let bits = dilate(
            fold(
                &pixels,
                nodata,
                source,
                (grid_width, grid_height),
                words_per_row,
            ),
            grid_width,
            grid_height,
        );

        // Nothing legitimately folds to nothing: a dataset exists because it holds data.
        // Trusting an empty grid would skip the dataset on every tile until the process
        // restarts, so treat it as a broken pyramid and keep reading the dataset.
        if bits.iter().all(|word| *word == 0) {
            eprintln!("hillshading {name}: footprint came out empty, never skipping");

            return None;
        }

        Some(Self {
            geo_transform: attempt(name, "reading the geo transform", dataset.geo_transform())?,
            raster_width,
            raster_height,
            grid_width,
            grid_height,
            words_per_row,
            bits,
        })
    }

    /// Whether any cell the bbox touches holds data. Rounds outward everywhere, so a
    /// tile is only rejected when the whole area it needs is empty.
    pub fn covers(&self, bbox: &Rect<f64>) -> bool {
        let window = PixelWindow::new(&self.geo_transform, bbox);

        let Some((x0, x1)) = cell_range(
            window.min_x(),
            window.max_x(),
            self.raster_width,
            self.grid_width,
        ) else {
            return false;
        };

        let Some((y0, y1)) = cell_range(
            window.min_y(),
            window.max_y(),
            self.raster_height,
            self.grid_height,
        ) else {
            return false;
        };

        (y0..y1).any(|row| self.row_has_data(row, x0, x1))
    }

    fn row_has_data(&self, row: usize, x0: usize, x1: usize) -> bool {
        let base = row * self.words_per_row;
        let first = x0 / 64;
        let last = (x1 - 1) / 64;

        (first..=last).any(|word_index| {
            let mut word = self.bits[base + word_index];

            if word_index == first {
                word &= u64::MAX << (x0 % 64);
            }

            if word_index == last {
                let end = x1 - word_index * 64;

                if end < 64 {
                    word &= (1 << end) - 1;
                }
            }

            word != 0
        })
    }
}

/// The level to build from, and its size; `None` for the band itself. Chosen by size
/// rather than by level, as pyramid depth varies (Sweden has 13 mask overviews, Finland
/// 8): the coarsest level that still resolves [`SOURCE_PER_CELL`] pixels per cell edge,
/// or the finest one that fits in memory when no level does both.
fn pick_source(band: &RasterBand) -> Option<(Option<usize>, (usize, usize))> {
    let mut affordable: Vec<(Option<usize>, (usize, usize))> = Vec::new();

    for index in 0..band.overview_count().unwrap_or(0).max(0) as usize {
        let Ok(overview) = band.overview(index) else {
            continue;
        };

        affordable.push((Some(index), overview.size()));
    }

    affordable.push((None, band.size()));

    affordable.retain(|(_, size)| size.0 * size.1 <= MAX_SOURCE_PIXELS);

    let wanted = GRID * SOURCE_PER_CELL;

    affordable
        .iter()
        .filter(|(_, size)| size.0 >= wanted && size.1 >= wanted)
        .min_by_key(|(_, size)| size.0 * size.1)
        .or_else(|| affordable.iter().max_by_key(|(_, size)| size.0 * size.1))
        .copied()
}

/// Sets the bit of every cell holding a pixel that is not `nodata`.
fn fold(
    pixels: &[u8],
    nodata: u8,
    (width, height): (usize, usize),
    (grid_width, grid_height): (usize, usize),
    words_per_row: usize,
) -> Vec<u64> {
    let mut bits = vec![0u64; words_per_row * grid_height];

    let columns: Vec<usize> = (0..width).map(|x| x * grid_width / width).collect();

    for y in 0..height {
        let base = (y * grid_height / height) * words_per_row;

        for (x, value) in pixels[y * width..(y + 1) * width].iter().enumerate() {
            if *value != nodata {
                let column = columns[x];

                bits[base + column / 64] |= 1 << (column % 64);
            }
        }
    }

    bits
}

/// Grows the set by [`DILATION`] cells in every direction. Insurance against what the
/// pyramid has already dropped at the level read: it costs about four percentage points
/// of acceptance (Sweden 47.5% -> 52.2%) and takes the last few misses to zero.
fn dilate(bits: Vec<u64>, grid_width: usize, grid_height: usize) -> Vec<u64> {
    let mut bits = bits;

    for _ in 0..DILATION {
        bits = grow(&bits, grid_width, grid_height);
    }

    bits
}

fn grow(bits: &[u64], grid_width: usize, grid_height: usize) -> Vec<u64> {
    let words_per_row = grid_width.div_ceil(64);

    // Keeps the bits past the last column clear, so the same grid always has the same
    // words however it was built.
    let tail = if grid_width.is_multiple_of(64) {
        u64::MAX
    } else {
        (1 << (grid_width % 64)) - 1
    };

    let mut wide = vec![0u64; bits.len()];

    for row in 0..grid_height {
        let base = row * words_per_row;

        for word in 0..words_per_row {
            let value = bits[base + word];

            // The neighbouring cell can sit in the neighbouring word.
            let from_left = if word > 0 { bits[base + word - 1] >> 63 } else { 0 };

            let from_right = if word + 1 < words_per_row {
                bits[base + word + 1] << 63
            } else {
                0
            };

            wide[base + word] = value | (value << 1) | (value >> 1) | from_left | from_right;
        }

        wide[base + words_per_row - 1] &= tail;
    }

    let mut out = wide.clone();

    for row in 0..grid_height {
        let base = row * words_per_row;

        for word in 0..words_per_row {
            if row > 0 {
                out[base + word] |= wide[base - words_per_row + word];
            }

            if row + 1 < grid_height {
                out[base + word] |= wide[base + words_per_row + word];
            }
        }
    }

    out
}

/// Half-open cell range covering `[px0, px1)` after clamping it to the raster, or
/// `None` when the window falls outside. Always at least one cell wide.
fn cell_range(px0: isize, px1: isize, raster: usize, grid: usize) -> Option<(usize, usize)> {
    let lo = px0.clamp(0, raster as isize) as usize;
    let hi = px1.clamp(0, raster as isize) as usize;

    if hi <= lo {
        return None;
    }

    let c0 = lo * grid / raster;
    let c1 = (hi * grid).div_ceil(raster).min(grid).max(c0 + 1);

    Some((c0, c1))
}

#[cfg(test)]
mod tests {
    use super::{Footprint, PixelWindow, cell_range, fold, grow};
    use geo::{Coord, Rect};

    /// North-up: the geo y axis runs against the pixel y axis, and the window has to
    /// come out ordered anyway.
    #[test]
    fn pixel_window_flips_y() {
        let gt = [0.0, 10.0, 0.0, 1000.0, 0.0, -10.0];

        let window = PixelWindow::new(&gt, &Rect::new((25.0, 105.0), (55.0, 205.0)));

        assert_eq!((window.min_x(), window.max_x()), (2, 6));
        assert_eq!((window.min_y(), window.max_y()), (79, 90));
    }

    #[test]
    fn cell_range_rounds_outward_and_clamps() {
        // 1000 pixels over 10 cells: 100 pixels per cell.
        assert_eq!(cell_range(0, 1000, 1000, 10), Some((0, 10)));
        assert_eq!(cell_range(-50, 50, 1000, 10), Some((0, 1)));
        assert_eq!(cell_range(950, 2000, 1000, 10), Some((9, 10)));

        // A window narrower than a cell still asks about the cell it lands in.
        assert_eq!(cell_range(105, 106, 1000, 10), Some((1, 2)));

        assert_eq!(cell_range(-100, 0, 1000, 10), None);
        assert_eq!(cell_range(1000, 1100, 1000, 10), None);
    }

    /// A single valid pixel has to set its cell, however many pixels the cell holds —
    /// this is what `Average` cannot promise.
    #[test]
    fn fold_keeps_a_lone_pixel() {
        let mut pixels = vec![0u8; 800 * 800];
        pixels[513 * 800 + 257] = 255;

        // Row 513 of 800 falls in grid row 2, column 257 in grid column 1.
        let bits = fold(&pixels, 0, (800, 800), (4, 4), 1);

        assert_eq!(bits, vec![0, 0, 1 << 1, 0]);
    }

    #[test]
    fn grow_reaches_one_cell_across_words() {
        // 3 rows of 128 cells, one set at row 1, column 64.
        let mut bits = vec![0u64; 6];
        bits[3] = 1;

        let out = grow(&bits, 128, 3);

        // Columns 63, 64 and 65 of every row, so bit 63 of the low word too.
        for row in 0..3 {
            assert_eq!(out[row * 2], 1 << 63, "row {row} low word");
            assert_eq!(out[row * 2 + 1], 0b11, "row {row} high word");
        }
    }

    /// One data cell at grid (2, 1) of a 4x4 grid over a 400x400 raster whose pixels
    /// are one geo unit wide, so geo x maps to pixel x and geo y to 400 - pixel y.
    /// Built without dilation, to test `covers` on its own.
    fn one_cell_footprint() -> Footprint {
        let mut bits = vec![0u64; 4];
        bits[1] = 1 << 2;

        Footprint {
            geo_transform: [0.0, 1.0, 0.0, 400.0, 0.0, -1.0],
            raster_width: 400,
            raster_height: 400,
            grid_width: 4,
            grid_height: 4,
            words_per_row: 1,
            bits,
        }
    }

    #[test]
    fn covers_only_where_the_mask_has_data() {
        let footprint = one_cell_footprint();

        // Inside the data cell: pixels x 200..300, y 100..200.
        assert!(footprint.covers(&Rect::new((210.0, 210.0), (220.0, 220.0))));

        // A sliver still inside it, after the outward rounding.
        assert!(footprint.covers(&Rect::new((250.0, 250.0), (250.1, 250.1))));

        // Another corner of the raster.
        assert!(!footprint.covers(&Rect::new((10.0, 10.0), (20.0, 20.0))));

        // The neighbouring cell on each axis.
        assert!(!footprint.covers(&Rect::new((210.0, 310.0), (220.0, 320.0))));
        assert!(!footprint.covers(&Rect::new((110.0, 210.0), (120.0, 220.0))));

        // The whole raster, and beyond it.
        assert!(footprint.covers(&Rect::new((-1000.0, -1000.0), (1000.0, 1000.0))));

        // Entirely outside.
        assert!(!footprint.covers(&Rect::new((500.0, 500.0), (600.0, 600.0))));
    }

    /// The bit scan has to mask both ends of the range, including across words.
    #[test]
    fn covers_masks_partial_words() {
        let grid = 130;
        let words_per_row = 3;

        let mut bits = vec![0u64; words_per_row];
        bits[1] = 1 << 5; // column 69

        let footprint = Footprint {
            geo_transform: [0.0, 1.0, 0.0, 130.0, 0.0, -1.0],
            raster_width: grid,
            raster_height: 1,
            grid_width: grid,
            grid_height: 1,
            words_per_row,
            bits,
        };

        let column = |x: f64| Rect::new(Coord { x, y: 129.0 }, Coord { x: x + 1.0, y: 130.0 });

        assert!(footprint.covers(&column(69.0)));
        assert!(!footprint.covers(&column(68.0)));
        assert!(!footprint.covers(&column(70.0)));

        // Ranges that end just before the set bit and start just after it.
        assert!(!footprint.covers(&Rect::new((0.0, 129.0), (69.0, 130.0))));
        assert!(!footprint.covers(&Rect::new((70.0, 129.0), (130.0, 130.0))));
        assert!(footprint.covers(&Rect::new((0.0, 129.0), (70.0, 130.0))));
    }
}

#[cfg(test)]
mod against_real_data {
    use super::Footprint;
    use gdal::Dataset;

    /// Prints the footprint of a real dataset in the form `scripts/verify-footprint.py
    /// --hash` prints it, so the implementation here can be checked against the model
    /// that script verifies. Ignored by default, as it needs a hillshading build:
    ///
    /// ```text
    /// FOOTPRINT_TIF=.../fi/final.tif cargo test against_real_data -- --ignored --nocapture
    /// ./scripts/verify-footprint.py --hash .../hillshading fi
    /// ```
    #[test]
    #[ignore = "needs a hillshading dataset on disk"]
    fn matches_the_model() {
        let path = std::env::var("FOOTPRINT_TIF").expect("FOOTPRINT_TIF names a final.tif");
        let dataset = Dataset::open(&path).expect("the dataset opens");
        let footprint = Footprint::build(&dataset, "check").expect("the dataset has a footprint");

        // FNV-1a over the words, little-endian, as the script hashes them.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;

        for word in &footprint.bits {
            for byte in word.to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x100_0000_01b3);
            }
        }

        println!(
            "grid {}x{} words {} set {} hash {hash:016x}",
            footprint.grid_width,
            footprint.grid_height,
            footprint.words_per_row,
            footprint.bits.iter().map(|word| word.count_ones()).sum::<u32>(),
        );
    }
}
