# Transform accuracy

kenro's `ST_Transform` is backed by [proj4rs] with a curated EPSG table
(see the README "Supported CRS" section). This page quantifies how closely
it matches the reference implementation, and — just as important — what the
reference itself is and is not.

## What was measured

`scripts/accuracy/generate.sh` transforms a lattice of points covering the
Japan region (122–146°E × 24–46°N, plus a finer in-zone lattice for plane
rectangular zone IX and a projected-coordinate lattice for the inverse
direction) through PostGIS, and commits the results as
`scripts/accuracy/reference.jsonl`. `cargo run --example accuracy_report`
runs the same points through kenro and reports the coordinate error in
meters (degree deltas scaled by the local metric factors).

Reference: **PostGIS 3.5.2 / PROJ 7.2.1** (postgis/postgis:17-3.5 Docker
image), **without Japanese datum grids**.

## Results

| pair | points | max (m) | mean (m) | p99 (m) |
|---|---|---|---|---|
| 2451→4326 (JGD2000 zone IX → WGS84) | 99 | 1.0e-4 | 9.5e-5 | 1.0e-4 |
| 4326→3857 (WGS84 → WebMercator) | 575 | 3.4e-9 | 1.4e-9 | 3.0e-9 |
| 4326→6677 (WGS84 → JGD2011 zone IX, in-zone) | 209 | 2.9e-8 | 2.6e-8 | 2.8e-8 |
| 4326→6677 (Japan-wide, incl. far out-of-zone) | 575 | 3.1e-8 | 2.6e-8 | 2.9e-8 |
| 4612→4326 (JGD2000 → WGS84) | 575 | 1.0e-4 | 9.5e-5 | 1.0e-4 |
| 4612→6668 (JGD2000 → JGD2011) | 575 | 1.5e-9 | 2.0e-10 | 1.4e-9 |
| 6668→4326 (JGD2011 → WGS84) | 575 | 1.5e-9 | 2.0e-10 | 1.4e-9 |

Summary: projection math (transverse Mercator, Mercator, UTM) agrees with
gridless PROJ at the **nanometer level**; pairs that route through the
JGD2000 `+towgs84` Helmert step agree at the **0.1 mm level**. Both are far
below any GIS use-case's noise floor.

CI runs `cargo run --example accuracy_report -- --check`, which fails if any
pair's max error exceeds its documented threshold (a regression gate for
proj4rs upgrades; thresholds are set roughly 10× above the measured max).

## What this does NOT claim — read before surveying anything

- The comparison is **kenro vs gridless PROJ**, not vs survey-grade truth.
  The PostGIS image ships no Japanese datum grids, and kenro's curated table
  models WGS84 / JGD2000 / JGD2011 as GRS80/WGS84-class ellipsoids with zero
  or absent Helmert shifts — so **datum transforms among them are identity**
  at this level, on both sides of the comparison.
- The real-world JGD2000 ↔ JGD2011 displacement (up to meters in Tōhoku
  after the 2011 earthquake, handled by GSI's parameter grids) is modeled by
  **neither side**. The same applies to Tokyo Datum (EPSG 4301), which kenro
  deliberately omits.
- For survey-grade work, use full PROJ with the official grids. kenro's
  transform is for the "web map / analysis / tiling" class of accuracy.

Axis-order note: kenro (like PostGIS's output) uses GIS-traditional
easting = x / northing = y, even though the EPSG registry defines the JGD
plane rectangular axes as northing-first.

[proj4rs]: https://github.com/3liz/proj4rs
