# Test fixtures

## mini.gpkg

A tiny GDAL-written GeoPackage (layer `parks`: two polygons and one point
around Tokyo, EPSG:4326) used by `tests/gpkg_rtree.rs` to prove interop with
externally produced GPB blobs — GDAL writes envelopes into the GPB headers
(exercising kenro's header fast path) and ships its own R-tree maintenance
triggers.

Regenerate with (GDAL 3.x):

```sh
ogr2ogr -f GPKG tests/fixtures/mini.gpkg mini.geojson -nln parks
```

where `mini.geojson` is:

```json
{
  "type": "FeatureCollection",
  "features": [
    {"type":"Feature","properties":{"name":"yoyogi"},"geometry":{"type":"Polygon","coordinates":[[[139.694,35.669],[139.700,35.669],[139.700,35.675],[139.694,35.675],[139.694,35.669]]]}},
    {"type":"Feature","properties":{"name":"ueno"},"geometry":{"type":"Polygon","coordinates":[[[139.770,35.712],[139.776,35.712],[139.776,35.718],[139.770,35.718],[139.770,35.712]]]}},
    {"type":"Feature","properties":{"name":"fountain"},"geometry":{"type":"Point","coordinates":[139.697,35.672]}}
  ]
}
```
