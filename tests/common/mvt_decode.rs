//! Minimal MVT decoder for golden verification only (never shipped).
//! Decodes a single-layer tile into the same normalized JSON shape the
//! generator (`scripts/golden/mvt_generate.py`) writes via the independent
//! `mapbox-vector-tile` implementation: `{name, extent, features: [{type,
//! coordinates, properties}]}` with raw Y-down integer coordinates.
#![allow(dead_code)]

use serde_json::{Value, json};

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    fn done(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn varint(&mut self) -> u64 {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = self.buf[self.pos];
            self.pos += 1;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return v;
            }
            shift += 7;
        }
    }

    /// (field, wire)
    fn key(&mut self) -> (u64, u64) {
        let k = self.varint();
        (k >> 3, k & 0x7)
    }

    fn bytes(&mut self) -> &'a [u8] {
        let len = self.varint() as usize;
        let out = &self.buf[self.pos..self.pos + len];
        self.pos += len;
        out
    }

    fn skip(&mut self, wire: u64) {
        match wire {
            0 => {
                self.varint();
            }
            1 => self.pos += 8,
            2 => {
                self.bytes();
            }
            5 => self.pos += 4,
            other => panic!("unsupported wire type {other}"),
        }
    }
}

fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

fn decode_value(buf: &[u8]) -> Value {
    let mut r = Reader::new(buf);
    while !r.done() {
        let (field, wire) = r.key();
        match field {
            1 => return Value::String(String::from_utf8(r.bytes().to_vec()).unwrap()),
            2 => {
                let mut b = [0u8; 4];
                b.copy_from_slice(&r.buf[r.pos..r.pos + 4]);
                return json!(f32::from_le_bytes(b) as f64);
            }
            3 => {
                let mut b = [0u8; 8];
                b.copy_from_slice(&r.buf[r.pos..r.pos + 8]);
                return json!(f64::from_le_bytes(b));
            }
            4 => return json!(r.varint() as i64),
            5 => return json!(r.varint()),
            6 => return json!(unzigzag(r.varint())),
            7 => return json!(r.varint() != 0),
            _ => r.skip(wire),
        }
    }
    Value::Null
}

/// Decode the packed geometry command stream into paths of (x, y).
fn decode_paths(cmds: &[u8]) -> Vec<Vec<(i64, i64)>> {
    let mut r = Reader::new(cmds);
    let (mut x, mut y) = (0i64, 0i64);
    let mut paths: Vec<Vec<(i64, i64)>> = Vec::new();
    while !r.done() {
        let c = r.varint();
        let (id, count) = (c & 0x7, c >> 3);
        match id {
            1 => {
                // MoveTo: each coordinate starts a new path (points collapse
                // multi-count MoveTo into one path per point).
                for _ in 0..count {
                    x += unzigzag(r.varint());
                    y += unzigzag(r.varint());
                    paths.push(vec![(x, y)]);
                }
            }
            2 => {
                for _ in 0..count {
                    x += unzigzag(r.varint());
                    y += unzigzag(r.varint());
                    paths.last_mut().expect("LineTo before MoveTo").push((x, y));
                }
            }
            7 => {
                let path = paths.last_mut().expect("ClosePath before MoveTo");
                let first = path[0];
                path.push(first);
            }
            other => panic!("unknown command id {other}"),
        }
    }
    paths
}

fn ring_area(ring: &[(i64, i64)]) -> i64 {
    let mut acc = 0i64;
    for w in ring.windows(2) {
        acc += w[0].0 * w[1].1 - w[1].0 * w[0].1;
    }
    acc
}

fn path_json(path: &[(i64, i64)]) -> Value {
    Value::Array(path.iter().map(|(x, y)| json!([x, y])).collect())
}

fn geometry_json(geom_type: u64, paths: Vec<Vec<(i64, i64)>>) -> (String, Value) {
    match geom_type {
        1 => {
            let pts: Vec<Value> = paths.iter().map(|p| json!([p[0].0, p[0].1])).collect();
            if pts.len() == 1 {
                ("Point".into(), pts.into_iter().next().unwrap())
            } else {
                ("MultiPoint".into(), Value::Array(pts))
            }
        }
        2 => {
            if paths.len() == 1 {
                ("LineString".into(), path_json(&paths[0]))
            } else {
                (
                    "MultiLineString".into(),
                    Value::Array(paths.iter().map(|p| path_json(p)).collect()),
                )
            }
        }
        3 => {
            // Group rings into polygons: positive area (spec winding) starts
            // a new polygon, negative rings are holes of the current one.
            let mut polys: Vec<Vec<Value>> = Vec::new();
            for path in &paths {
                if ring_area(path) >= 0 || polys.is_empty() {
                    polys.push(vec![path_json(path)]);
                } else {
                    polys.last_mut().unwrap().push(path_json(path));
                }
            }
            if polys.len() == 1 {
                (
                    "Polygon".into(),
                    Value::Array(polys.into_iter().next().unwrap()),
                )
            } else {
                (
                    "MultiPolygon".into(),
                    Value::Array(polys.into_iter().map(Value::Array).collect()),
                )
            }
        }
        other => panic!("unknown geometry type {other}"),
    }
}

/// Decode a kenro/PostGIS-style single-layer tile into normalized JSON.
pub fn decode_tile(tile: &[u8]) -> Value {
    let mut r = Reader::new(tile);
    let mut layer_bytes: Option<&[u8]> = None;
    while !r.done() {
        let (field, wire) = r.key();
        if field == 3 && wire == 2 {
            assert!(layer_bytes.is_none(), "multiple layers in tile");
            layer_bytes = Some(r.bytes());
        } else {
            r.skip(wire);
        }
    }
    let mut r = Reader::new(layer_bytes.expect("no layer in tile"));

    let mut name = String::new();
    let mut extent = 4096u64;
    let mut version = 0u64;
    let mut keys: Vec<String> = Vec::new();
    let mut values: Vec<Value> = Vec::new();
    let mut feature_msgs: Vec<&[u8]> = Vec::new();
    while !r.done() {
        let (field, wire) = r.key();
        match field {
            1 => name = String::from_utf8(r.bytes().to_vec()).unwrap(),
            2 => feature_msgs.push(r.bytes()),
            3 => keys.push(String::from_utf8(r.bytes().to_vec()).unwrap()),
            4 => values.push(decode_value(r.bytes())),
            5 => extent = r.varint(),
            15 => version = r.varint(),
            _ => r.skip(wire),
        }
    }
    assert_eq!(version, 2, "layer version");

    let features: Vec<Value> = feature_msgs
        .iter()
        .map(|msg| {
            let mut r = Reader::new(msg);
            let mut tags: Vec<u64> = Vec::new();
            let mut geom_type = 0u64;
            let mut cmds: &[u8] = &[];
            while !r.done() {
                let (field, wire) = r.key();
                match field {
                    2 => {
                        let mut pr = Reader::new(r.bytes());
                        while !pr.done() {
                            tags.push(pr.varint());
                        }
                    }
                    3 => geom_type = r.varint(),
                    4 => cmds = r.bytes(),
                    _ => r.skip(wire),
                }
            }
            let mut props = serde_json::Map::new();
            for pair in tags.chunks(2) {
                props.insert(
                    keys[pair[0] as usize].clone(),
                    values[pair[1] as usize].clone(),
                );
            }
            let (ty, coordinates) = geometry_json(geom_type, decode_paths(cmds));
            json!({ "type": ty, "coordinates": coordinates, "properties": props })
        })
        .collect();

    json!({ "name": name, "extent": extent, "features": features })
}
