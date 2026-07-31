# Transform accuracy

kenro's `ST_Transform` is backed by [proj4rs] with a built-in table of
globally-defined systems (see the README "Supported CRS" section). This page
quantifies how closely it matches the reference implementation, and — just
as important — what the reference itself is and is not.

## What was measured

`scripts/accuracy/generate.sh` transforms global point lattices (a worldwide
lattice for Web Mercator, in-zone and far-out-of-zone lattices for a
northern UTM zone, a southern-hemisphere UTM zone for the false-northing
path, and a projected-coordinate lattice for the inverse direction) through
PostGIS, and commits the results as `scripts/accuracy/reference.jsonl`.
`cargo run --example accuracy_report` runs the same points through kenro and
reports the coordinate error in meters (degree deltas scaled by the local
metric factors).

Reference: **PostGIS 3.5.2 / PROJ 7.2.1** (postgis/postgis:17-3.5 Docker
image), **without datum grids**.

## Results

| pair | points | max (m) | mean (m) | p99 (m) |
|---|---|---|---|---|
| 4326→3857 (WGS84 → WebMercator, worldwide) | 3827 | 1.2e-8 | 2.0e-9 | 1.1e-8 |
| 4326→32633 (WGS84 → UTM 33N, in-zone) | 234 | 1.9e-9 | 5.0e-10 | 1.5e-9 |
| 4326→32633 (far out-of-zone) | 352 | 2.0e-9 | 5.1e-10 | 1.5e-9 |
| 4326→32756 (WGS84 → UTM 56S, southern hemisphere) | 234 | 2.0e-9 | 4.6e-10 | 1.4e-9 |
| 32633→4326 (inverse, projected lattice) | 70 | 1.6e-9 | 2.5e-10 | 1.6e-9 |

Summary: the projection math (transverse Mercator / UTM, Mercator) agrees
with gridless PROJ at the **nanometer level** — far below any GIS use-case's
noise floor.

CI runs `cargo run --example accuracy_report -- --check`, which fails if any
pair's max error exceeds its documented threshold (a regression gate for
proj4rs upgrades; thresholds are set roughly 10× above the measured max).

## What this does NOT claim — read before surveying anything

- The comparison is **kenro vs gridless PROJ**, not vs survey-grade truth.
- National and regional systems resolve through the `crs-full` feature (the
  `crs-definitions` registry, generated from proj4js definitions). Their
  accuracy depends on those definitions — typically the same
  projection-math-plus-Helmert class as above, but **without datum grids**:
  national datum modernizations and earthquake-displacement models (NADCON,
  NTv2, Japan's GSI grids, …) are applied by **neither side** of this
  comparison.
- For survey-grade work, use full PROJ with the official grids. kenro's
  transform is for the "web map / analysis / tiling" class of accuracy.

Axis-order note: kenro (like PostGIS's output) always uses GIS-traditional
easting = x / northing = y, even where the EPSG registry defines a
northing-first axis order.

[proj4rs]: https://github.com/3liz/proj4rs
