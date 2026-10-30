# Place labels per country

Why `MAPRENDER_PLACE_TYPE_OVERRIDES` exists, how to decide what to put in it, and what we
measured while doing so. Background: [issue #76](https://github.com/FreemapSlovakia/freemap-outdoor-map/issues/76).

## The problem

`place=*` means different things in different countries:

- a Croatian `place=village` is an official *naselje* — median population **149**, and 79 % of
  them are under 500 inhabitants; a Slovak `place=village` is usually an *obec*;
- a Polish `place=hamlet` is often a *przysiółek*, a part of a village;
- an Austrian `place=hamlet` is a *Rotte* or *Weiler* of a few houses.

One zoom ladder for all of them therefore produces a wall of labels in some countries and a
well-balanced map in others. Measured against Slovakia, for which the style is tuned
(57 villages + 39 hamlets per 1000 km² of land), Croatia has 3.6× the label load, Poland 3.5×
and Slovenia 3.4×.

## The mechanism

Every labelled place type has a default rule — a minimum zoom and a named style — in
`PLACE_DEFAULTS` in [`place_names.rs`](../src/render/layers/place_names.rs). The SQL type
filter is generated from those zooms, so a type the layer never draws is not even fetched.

`MAPRENDER_PLACE_TYPE_OVERRIDES` replaces that pair per country:

```
<country>:<rule>[,<rule>…][;<country>:…]        rule = <from>[@<population>]=<to>
```

- **country** — lowercase ISO 3166-1 alpha-2 code, or `*` for every country without an entry
  of its own (which also covers places outside every country polygon). A country's own rule
  for a type *replaces* the `*` one; types it does not mention still follow `*`.
- **target** — `z<zoom>[/<style>]`, or `-` to not label the type at all. Without a style the
  source type keeps its own.
- **`@<population>`** — the rule matches only places below that population. **An untagged
  population counts as 0**, so it is demoted too. Several rules for one type form tiers: the
  lowest matching population wins, a rule without `@` takes the rest. Order in the string is
  irrelevant.

Styles, all of which include their capitalization:

| style | size | case | | style | size |
|---|---|---|---|---|---|
| `xxl` | 1.20 | CAPS | | `s` | 0.45 |
| `xl` | 0.80 | CAPS | | `xs` | 0.40 |
| `l` | 0.55 | CAPS | | `xxs` | 0.35 |
| `m` | 0.50 | | | | |

Defaults: city `z8/xxl`, town `z9/xl`, village `z11/l`, hamlet · allotments · suburb `z12/m`,
isolated_dwelling · quarter `z14/s`, neighbourhood `z15/xs`, farm · borough · square `z16/xxs`.
`island`/`islet` are outside the scheme — they appear once the mapped area is big enough.

A rule may only **postpone** a type, never make it appear earlier: the SQL fetches types by
their original type per zoom, so a promoted place would not even be in the result set.
Startup rejects anything else, along with unknown styles, zooms past z17 and duplicate tiers.

The country of a place is resolved by point-in-polygon against the `countries` table — see
[Country polygons](../README.md#country-polygons). Configuring overrides without that table
fails at startup.

## Two traps

**A demotion only thins a view if its target zoom is above the zoom you are looking at.**
Hamlets are drawn from z12, so in a z12 view `village=z12/m` merely shrinks labels and removes
none. This cost us a round of tuning in Croatia: the first `village=hamlet` rule looked like it
did nothing.

**`@N` is only as good as the country's population coverage.** It is excellent in Croatia
(96 % of villages), useless for hamlets nearly everywhere (1–3 %), and misleading in between:
in Slovenia only 33 % of villages are tagged, so `village@500=…` demotes the two-thirds that
are untagged regardless of their actual size.

## Measuring a country

Counts come from Overpass, land area from the `countries` table intersected with the land
polygons — territorial waters would otherwise dilute maritime countries such as Croatia:

```sql
SELECT c.country,
       round((SUM(ST_Area(ST_Transform(ST_Intersection(c.geometry, l.geometry), 4326)::geography)) / 1e6)::numeric) AS land_km2
FROM countries c JOIN land_z5_7 l ON c.geometry && l.geometry AND ST_Intersects(c.geometry, l.geometry)
GROUP BY c.country;
```

That reproduces official figures closely (SK 49 029 vs 49 035 km², DE 357 579 vs 357 592).

[`place-stats.py`](./place-stats.py) crawls the counts; results as of **2026-08-17** are in
[`place-density.csv`](./place-density.csv) (43 countries). Two caveats for re-running it:

- France and Germany time out with all eight counts in one request — split them into two
  requests, as the script's per-country query does not do automatically;
- country-wide `(if:number(t["population"])>=N)` filters also time out. Use a bbox around the
  region you care about instead, which is more informative anyway (see Austria below).

## What the numbers said

Per 1000 km² of land, ranked by villages + hamlets; Slovakia is 96.4 (57.3 + 39.1):

| | v/1000 | h/1000 | ×SK | pop % on villages / hamlets |
|---|---|---|---|---|
| HR | 111.4 | 234.6 | 3.6 | 96 / 2 |
| PL | 151.8 | 182.0 | 3.5 | 26 / 1 |
| SI | 235.6 | 94.7 | 3.4 | 33 / 13 |
| LT | 292.4 | 24.4 | 3.3 | **100 / 100** |
| LU | 208.5 | 80.4 | 3.0 | 83 / 33 |
| ME | 71.1 | 213.1 | 2.9 | 21 / 1 |
| AT | 72.3 | 189.5 | 2.7 | 86 / 46 |
| CH | 86.5 | 173.2 | 2.7 | 70 / 1 |
| IT | 49.1 | 168.2 | 2.3 | 62 / 25 |
| CZ | 140.0 | 72.9 | 2.2 | 88 / 64 |
| BY | 23.6 | 184.8 | 2.2 | 98 / 14 |
| DK · PT | 36.4 · 38.4 | 164.2 · 161.2 | 2.1 | 70 / 46 · 59 / 14 |
| BA · BE · XK | 98.7 · 96.7 · 138.7 | 90.0 · 89.3 · 41.1 | 1.9 | 20 / 3 · 14 / 1 · 12 / 4 |
| **SK** | **57.3** | **39.1** | **1.0** | 94 / 1 |

Lithuania is the one country where a population rule would be exact rather than a proxy.
Nobody has complained about Austria, Switzerland or Czechia despite their ranks — **density
ranks tagging, not perceived clutter**, and terrain spreads Alpine hamlets out. The proper fix
for that is [issue #80](https://github.com/FreemapSlovakia/freemap-outdoor-map/issues/80):
derive the minimum zoom from how much room a label has, rather than from its country.

Austria showed both failure modes of the national average at once: nationally 72 villages per
1000 km² looks harmless, but the Krems–St. Pölten–Tulln lowlands measure **172.5** — while its
hamlets, at 190 nationally, were caught by the average just fine. Always check a dense region's
bbox before settling a threshold.

## Current settings

The tuned rules live in the deployment's `.env`; [`.env.sample`](../.env.sample) documents the
format. As of 2026-08-17 the starting points were:

```
hr:village@100=z14/s,village@500=z12/m,hamlet=z16/xxs
si:village@20=z14/s,village@500=z12/m,hamlet=z15/xs
pl:village@300=z12/m,hamlet=z14/s
at:village@100=z14/s,village@300=z13/m,hamlet@100=z14/s
it:town@15000=z11/l,village@5000=z12/m,hamlet=z14/s
```

Italy is the one where towns crowd too: in the Po plain around Milan a `town` is any *comune*
of roughly 10 000 or more — 207 of them in 20 835 km² of land (8.4–10.6°E, 44.9–46.0°N), 3.4×
Slovakia's density — and they fill z9–z10 on their own. Villages there are tagged with a
population on 64 %, median 1 670, so the 1000 and 3000 thresholds hardly thinned z11; 5000 does,
at the cost of an almost empty plain around Lodi at z11. The Brianza north of Milan stays dense
at z11 with any threshold — its *comuni* really have 5 000–10 000 inhabitants.

After changing them, invalidate cached tiles from the lowest zoom a rule touches — z9 when it
postpones towns, z11 otherwise.
