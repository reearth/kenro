//! Hand-rolled Mapbox Vector Tile encoder (spec 2.1, layer version 2).
//!
//! Produces a `Tile` protobuf containing exactly one layer, the same output
//! shape PostGIS's `ST_AsMVT` emits — tiles for multiple layers can be built
//! by concatenating the byte strings. Input geometries must already be in
//! integer tile coordinates (Y down), i.e. the output of `ST_AsMVTGeom`.
//!
//! Written by hand because the protobuf surface is tiny (varints, zigzag,
//! length-delimited fields, command integers) and a prost/protoc dependency
//! would dominate the wasm size budget.

use std::collections::HashMap;

use geo_types::{Geometry, LineString, Polygon};

use crate::error::{Error, Result};

/// A typed MVT property value (`Value` message in the spec).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Double(f64),
    Int(i64),
    Bool(bool),
}

/// One feature: a tile-space geometry plus its properties.
pub struct Feature {
    pub geometry: Geometry<f64>,
    pub props: Vec<(String, Value)>,
}

// ---- protobuf wire primitives ----

fn write_varint(buf: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            buf.push(byte);
            break;
        }
        buf.push(byte | 0x80);
    }
}

fn write_key(buf: &mut Vec<u8>, field: u64, wire: u64) {
    write_varint(buf, (field << 3) | wire);
}

fn write_len_field(buf: &mut Vec<u8>, field: u64, bytes: &[u8]) {
    write_key(buf, field, 2);
    write_varint(buf, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

fn write_varint_field(buf: &mut Vec<u8>, field: u64, v: u64) {
    write_key(buf, field, 0);
    write_varint(buf, v);
}

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn command(id: u64, count: u64) -> u64 {
    (id & 0x7) | (count << 3)
}

// ---- geometry command encoding ----

const MOVE_TO: u64 = 1;
const LINE_TO: u64 = 2;
const CLOSE_PATH: u64 = 7;

struct Cursor {
    x: i64,
    y: i64,
}

impl Cursor {
    fn delta(&mut self, x: f64, y: f64) -> (u64, u64) {
        let (xi, yi) = (x.round() as i64, y.round() as i64);
        let d = (zigzag(xi - self.x), zigzag(yi - self.y));
        self.x = xi;
        self.y = yi;
        d
    }
}

/// Signed ring area by the surveyor's formula in tile coordinates — the
/// spec's winding test (exterior > 0, interior < 0 with Y down).
fn ring_area(ring: &LineString<f64>) -> f64 {
    let pts = &ring.0;
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut acc = 0.0;
    for i in 0..n - 1 {
        acc += pts[i].x * pts[i + 1].y - pts[i + 1].x * pts[i].y;
    }
    acc / 2.0
}

fn encode_points(cmds: &mut Vec<u64>, cursor: &mut Cursor, pts: &[geo_types::Point<f64>]) {
    cmds.push(command(MOVE_TO, pts.len() as u64));
    for p in pts {
        let (dx, dy) = cursor.delta(p.x(), p.y());
        cmds.push(dx);
        cmds.push(dy);
    }
}

fn encode_linestring(cmds: &mut Vec<u64>, cursor: &mut Cursor, ls: &LineString<f64>) {
    cmds.push(command(MOVE_TO, 1));
    let (dx, dy) = cursor.delta(ls.0[0].x, ls.0[0].y);
    cmds.push(dx);
    cmds.push(dy);
    cmds.push(command(LINE_TO, (ls.0.len() - 1) as u64));
    for c in &ls.0[1..] {
        let (dx, dy) = cursor.delta(c.x, c.y);
        cmds.push(dx);
        cmds.push(dy);
    }
}

/// Encode one closed ring (first == last coord), reoriented so its surveyor
/// area matches `exterior`.
fn encode_ring(cmds: &mut Vec<u64>, cursor: &mut Cursor, ring: &LineString<f64>, exterior: bool) {
    let mut ring = ring.clone();
    if (ring_area(&ring) > 0.0) != exterior {
        ring.0.reverse();
    }
    let open = &ring.0[..ring.0.len() - 1];
    cmds.push(command(MOVE_TO, 1));
    let (dx, dy) = cursor.delta(open[0].x, open[0].y);
    cmds.push(dx);
    cmds.push(dy);
    cmds.push(command(LINE_TO, (open.len() - 1) as u64));
    for c in &open[1..] {
        let (dx, dy) = cursor.delta(c.x, c.y);
        cmds.push(dx);
        cmds.push(dy);
    }
    cmds.push(command(CLOSE_PATH, 1));
}

fn encode_polygon(cmds: &mut Vec<u64>, cursor: &mut Cursor, poly: &Polygon<f64>) {
    encode_ring(cmds, cursor, poly.exterior(), true);
    for interior in poly.interiors() {
        encode_ring(cmds, cursor, interior, false);
    }
}

/// `(geom_type, command_stream)` for a tile-space geometry.
fn encode_geometry(func: &'static str, g: &Geometry<f64>) -> Result<(u64, Vec<u64>)> {
    let mut cmds = Vec::new();
    let mut cursor = Cursor { x: 0, y: 0 };
    let geom_type = match g {
        Geometry::Point(p) => {
            encode_points(&mut cmds, &mut cursor, &[*p]);
            1
        }
        Geometry::MultiPoint(mp) => {
            encode_points(&mut cmds, &mut cursor, &mp.0);
            1
        }
        Geometry::LineString(ls) => {
            encode_linestring(&mut cmds, &mut cursor, ls);
            2
        }
        Geometry::MultiLineString(mls) => {
            for ls in &mls.0 {
                encode_linestring(&mut cmds, &mut cursor, ls);
            }
            2
        }
        Geometry::Polygon(p) => {
            encode_polygon(&mut cmds, &mut cursor, p);
            3
        }
        Geometry::MultiPolygon(mp) => {
            for p in &mp.0 {
                encode_polygon(&mut cmds, &mut cursor, p);
            }
            3
        }
        _ => {
            return Err(Error::Unsupported {
                func,
                reason: format!(
                    "{} geometries cannot be encoded into an MVT feature; \
                     pass the geometry through ST_AsMVTGeom first",
                    crate::geom::wkt_type_name(g)
                ),
            });
        }
    };
    Ok((geom_type, cmds))
}

// ---- Value / layer assembly ----

fn encode_value(v: &Value) -> Vec<u8> {
    let mut buf = Vec::new();
    match v {
        Value::Str(s) => write_len_field(&mut buf, 1, s.as_bytes()),
        Value::Double(d) => {
            write_key(&mut buf, 3, 1);
            buf.extend_from_slice(&d.to_le_bytes());
        }
        Value::Int(i) if *i >= 0 => write_varint_field(&mut buf, 4, *i as u64),
        Value::Int(i) => write_varint_field(&mut buf, 6, zigzag(*i)),
        Value::Bool(b) => write_varint_field(&mut buf, 7, u64::from(*b)),
    }
    buf
}

/// Encode a single-layer MVT tile from tile-space features.
pub fn encode_tile(
    func: &'static str,
    layer_name: &str,
    extent: u32,
    features: &[Feature],
) -> Result<Vec<u8>> {
    let mut keys: Vec<&str> = Vec::new();
    let mut key_index: HashMap<&str, u64> = HashMap::new();
    let mut values: Vec<Vec<u8>> = Vec::new();
    let mut value_index: HashMap<Vec<u8>, u64> = HashMap::new();

    let mut feature_msgs: Vec<Vec<u8>> = Vec::new();
    for feature in features {
        let (geom_type, cmds) = encode_geometry(func, &feature.geometry)?;
        let mut tags = Vec::new();
        for (key, value) in &feature.props {
            let ki = *key_index.entry(key.as_str()).or_insert_with(|| {
                keys.push(key.as_str());
                (keys.len() - 1) as u64
            });
            let encoded = encode_value(value);
            let vi = match value_index.get(&encoded) {
                Some(&i) => i,
                None => {
                    let i = values.len() as u64;
                    value_index.insert(encoded.clone(), i);
                    values.push(encoded);
                    i
                }
            };
            tags.push(ki);
            tags.push(vi);
        }
        let mut msg = Vec::new();
        if !tags.is_empty() {
            let mut packed = Vec::new();
            for t in &tags {
                write_varint(&mut packed, *t);
            }
            write_len_field(&mut msg, 2, &packed);
        }
        write_varint_field(&mut msg, 3, geom_type);
        let mut packed = Vec::new();
        for c in &cmds {
            write_varint(&mut packed, *c);
        }
        write_len_field(&mut msg, 4, &packed);
        feature_msgs.push(msg);
    }

    let mut layer = Vec::new();
    write_varint_field(&mut layer, 15, 2); // version
    write_len_field(&mut layer, 1, layer_name.as_bytes());
    for msg in &feature_msgs {
        write_len_field(&mut layer, 2, msg);
    }
    for key in &keys {
        write_len_field(&mut layer, 3, key.as_bytes());
    }
    for value in &values {
        write_len_field(&mut layer, 4, value);
    }
    write_varint_field(&mut layer, 5, u64::from(extent));

    let mut tile = Vec::new();
    write_len_field(&mut tile, 3, &layer);
    Ok(tile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_boundaries() {
        let mut buf = Vec::new();
        write_varint(&mut buf, 0);
        write_varint(&mut buf, 127);
        write_varint(&mut buf, 128);
        write_varint(&mut buf, 300);
        assert_eq!(buf, [0x00, 0x7f, 0x80, 0x01, 0xac, 0x02]);
    }

    #[test]
    fn zigzag_matches_spec() {
        assert_eq!(zigzag(0), 0);
        assert_eq!(zigzag(-1), 1);
        assert_eq!(zigzag(1), 2);
        assert_eq!(zigzag(-2), 3);
    }

    #[test]
    fn spec_example_polygon_commands() {
        // The spec 4.3.5.2 example: MoveTo(3,6), LineTo(8,12), LineTo(20,34),
        // ClosePath.
        let ring = LineString::from(vec![(3.0, 6.0), (8.0, 12.0), (20.0, 34.0), (3.0, 6.0)]);
        let poly = Polygon::new(ring, vec![]);
        let (ty, cmds) = encode_geometry("test", &Geometry::Polygon(poly)).unwrap();
        assert_eq!(ty, 3);
        assert_eq!(cmds, [9, 6, 12, 18, 10, 12, 24, 44, 15]);
    }
}
