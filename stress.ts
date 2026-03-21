// deno run --allow-net stress.ts [iterations] [concurrency]
//
// Loops over multiple zoom levels (z10–z18) and several regions in Slovakia,
// reporting timing + cumulative tile count so you can correlate with memory
// watched externally:
//   watch -n1 'cat /proc/$(pidof freemap-outdoor-map)/status | grep VmRSS'

export {};

const base = "http://localhost:3050";
const VARIANT = ""; // "" for main, "/kst" for KST variant
const SCALE = "@2x";

const TOTAL_ITERATIONS = Number(Deno.args[0] ?? 100);
const CONCURRENCY = Number(Deno.args[1] ?? 64);

// Multiple regions across Slovakia to exercise different feature sets:
//   center  – mixed landscape, place names, routes
//   tatras  – dense contours, hillshading, alpine POIs, hiking routes
//   west    – Bratislava surroundings, urban POIs, roads
//   east    – Košice area, varied terrain
//
// Each entry is the top-left corner of the rendered grid at that zoom.
// Tile coords computed for the named lat/lon:
//   x = floor((lon+180)/360 * 2^z)
//   y = floor((1 - ln(tan(lat°)+sec(lat°))/π) / 2 * 2^z)
const REGIONS: Record<string, { z: number; x: number; y: number }[]> = {
  center: [
    { z: 10, x: 561,    y: 353   },
    { z: 11, x: 1123,   y: 706   },
    { z: 12, x: 2246,   y: 1412  },
    { z: 13, x: 4492,   y: 2824  },
    { z: 14, x: 8985,   y: 5649  },
    { z: 15, x: 17970,  y: 11298 },
    { z: 16, x: 35940,  y: 22596 },
    { z: 17, x: 71880,  y: 45192 },
    { z: 18, x: 143760, y: 90384 },
  ],
  tatras: [
    // ~49.17°N 20.08°E  High Tatras – maximum contour/hillshading stress
    { z: 12, x: 2271,   y: 1394  },
    { z: 13, x: 4543,   y: 2788  },
    { z: 14, x: 9086,   y: 5577  },
    { z: 15, x: 18172,  y: 11154 },
    { z: 16, x: 36344,  y: 22308 },
    { z: 17, x: 72688,  y: 44616 },
    { z: 18, x: 145376, y: 89232 },
  ],
  west: [
    // ~48.15°N 17.12°E  Bratislava – dense urban POIs, roads, place names
    { z: 12, x: 2228,   y: 1420  },
    { z: 13, x: 4456,   y: 2840  },
    { z: 14, x: 8912,   y: 5680  },
    { z: 15, x: 17824,  y: 11360 },
    { z: 16, x: 35648,  y: 22720 },
    { z: 17, x: 71296,  y: 45440 },
    { z: 18, x: 142592, y: 90880 },
  ],
  east: [
    // ~48.72°N 21.26°E  Košice area – varied terrain, rivers
    { z: 12, x: 2284,   y: 1406  },
    { z: 13, x: 4568,   y: 2812  },
    { z: 14, x: 9136,   y: 5625  },
    { z: 15, x: 18272,  y: 11250 },
    { z: 16, x: 36544,  y: 22500 },
    { z: 17, x: 73088,  y: 45000 },
    { z: 18, x: 146176, y: 90000 },
  ],
};

// Larger grids at low zoom (cheap), smaller at very high zoom (expensive).
function gridSize(z: number): { w: number; h: number } {
  if (z <= 12) return { w: 6, h: 6 };
  if (z <= 14) return { w: 5, h: 5 };
  if (z <= 16) return { w: 4, h: 4 };
  return { w: 3, h: 3 }; // z17–z18: expensive, keep grid smaller
}

// ── helpers ──────────────────────────────────────────────────────────────────

const buildUrl = (z: number, x: number, y: number) =>
  `${base}${VARIANT}/${z}/${x}/${y}${SCALE}`;

function buildGrid(z: number, ox: number, oy: number): string[] {
  const { w, h } = gridSize(z);
  const urls: string[] = [];
  for (let dy = 0; dy < h; dy++) {
    for (let dx = 0; dx < w; dx++) {
      urls.push(buildUrl(z, ox + dx, oy + dy));
    }
  }
  return urls;
}

async function fetchOne(url: string): Promise<void> {
  const res = await fetch(url);
  if (!res.ok) {
    await res.body?.cancel();
    throw new Error(`HTTP ${res.status} ${url}`);
  }
  await res.arrayBuffer();
}

async function runBatch(
  urls: string[],
  concurrency: number,
): Promise<{ ok: number; failed: number; ms: number }> {
  const t0 = performance.now();
  let ok = 0;
  let failed = 0;

  for (let i = 0; i < urls.length; i += concurrency) {
    const chunk = urls.slice(i, i + concurrency);
    const results = await Promise.allSettled(chunk.map(fetchOne));
    for (const r of results) {
      if (r.status === "fulfilled") ok++;
      else {
        failed++;
        console.error("  FAIL:", (r as PromiseRejectedResult).reason);
      }
    }
  }

  return { ok, failed, ms: Math.round(performance.now() - t0) };
}

// ── main ─────────────────────────────────────────────────────────────────────

const allOrigins = Object.entries(REGIONS).flatMap(([region, origins]) =>
  origins.map((o) => ({ ...o, region }))
);

const tilesPerIter = allOrigins.reduce((sum, { z }) => {
  const { w, h } = gridSize(z);
  return sum + w * h;
}, 0);

console.log(`Stress test: ${TOTAL_ITERATIONS} iterations, concurrency=${CONCURRENCY}`);
console.log(`Regions: ${Object.keys(REGIONS).join(", ")}`);
console.log(`Zoom levels: ${[...new Set(allOrigins.map((o) => o.z))].join(",")}`);
console.log(`Tiles per iteration: ${tilesPerIter}`);
console.log();

let totalTiles = 0;
const t0Total = performance.now();

for (let iter = 1; iter <= TOTAL_ITERATIONS; iter++) {
  const iterUrls: string[] = [];

  for (const { z, x, y } of allOrigins) {
    iterUrls.push(...buildGrid(z, x, y));
  }

  // Shuffle so requests across regions and zooms are interleaved,
  // keeping all workers busy rather than serialising by region.
  for (let i = iterUrls.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [iterUrls[i], iterUrls[j]] = [iterUrls[j], iterUrls[i]];
  }

  const { ok, failed, ms } = await runBatch(iterUrls, CONCURRENCY);
  totalTiles += ok;

  const elapsed = Math.round((performance.now() - t0Total) / 1000);
  const ratePerSec = Math.round(ok / (ms / 1000));
  console.log(
    `iter ${String(iter).padStart(3)}/${TOTAL_ITERATIONS}` +
      `  tiles=${String(ok).padStart(4)} (${failed} failed)` +
      `  ${String(ratePerSec).padStart(4)} t/s` +
      `  iter_ms=${String(ms).padStart(5)}` +
      `  total=${String(totalTiles).padStart(7)}` +
      `  elapsed=${elapsed}s`,
  );

  // Brief pause so memory monitor can sample between iterations.
  await new Promise((r) => setTimeout(r, 200));
}

const totalMs = Math.round(performance.now() - t0Total);
console.log(
  `\nDone: ${totalTiles} tiles in ${totalMs}ms (${Math.round(totalTiles / (totalMs / 1000))} tiles/s)`,
);
