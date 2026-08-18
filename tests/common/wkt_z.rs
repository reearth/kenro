//! A narrow WKT-with-Z reader for the golden harnesses.
//!
//! The vectors carry operands as `POINT Z (…)`, `POLYHEDRALSURFACE Z(…)` and
//! friends, which kenro's `ST_GeomFromText` refuses by design — so the harness
//! builds ISO WKB itself, which is how a 3D geometry reaches kenro in practice
//! anyway (written by GDAL, QGIS or a CityGML importer).
//!
//! Deliberately narrow: exactly the types `scripts/golden/threed*.sql` write.
//! A general WKT parser is `ST_GeomFromText`'s job.
/// `POINT Z (…)`, `LINESTRING Z (…)`, `POLYGON Z ((…))`, the MULTI\* forms,
/// `POLYHEDRALSURFACE Z(…)`, `TIN Z(…)`, `TRIANGLE Z(…)`, and `… EMPTY`.
pub fn to_wkb(wkt: &str) -> Option<Vec<u8>> {
    let wkt = wkt.trim();
    let upper = wkt.to_ascii_uppercase();
    let head = upper.split(['(', ' ']).next().unwrap_or("");
    if upper.ends_with("EMPTY") {
        // An empty geometry of the named type: count 0, no coordinates.
        let base = match head {
            "POINT" => return None, // no vector uses POINT EMPTY
            "LINESTRING" => 2u32,
            "POLYGON" => 3,
            _ => return None,
        };
        let mut v = vec![0x01u8];
        v.extend_from_slice(&(1000 + base).to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        return Some(v);
    }
    // The coordinate text is everything from the first '(' to the last ')'.
    let body = &wkt[wkt.find('(')?..=wkt.rfind(')')?];
    match head {
        "POINT" => Some(point(nums(body)?)),
        "LINESTRING" => Some(run(2, &coords(body)?)),
        "POLYGON" => Some(rings(3, body)?),
        "TRIANGLE" => Some(rings(17, body)?),
        "MULTIPOINT" => {
            let parts = split_parts(&body[1..body.len() - 1])?;
            let mut v = header(4, parts.len());
            for p in parts {
                v.extend_from_slice(&point(nums(&p)?));
            }
            Some(v)
        }
        "MULTILINESTRING" => {
            let parts = split_parts(&body[1..body.len() - 1])?;
            let mut v = header(5, parts.len());
            for p in parts {
                v.extend_from_slice(&run(2, &coords(&p)?));
            }
            Some(v)
        }
        "MULTIPOLYGON" => {
            let parts = split_parts(&body[1..body.len() - 1])?;
            let mut v = header(6, parts.len());
            for p in parts {
                v.extend_from_slice(&rings(3, &p)?);
            }
            Some(v)
        }
        "POLYHEDRALSURFACE" | "TIN" => {
            let base = if head == "TIN" { 16 } else { 15 };
            let patch = if head == "TIN" { 17 } else { 3 };
            let parts = split_parts(&body[1..body.len() - 1])?;
            let mut v = header(base, parts.len());
            for p in parts {
                v.extend_from_slice(&rings(patch, &p)?);
            }
            Some(v)
        }
        _ => None,
    }
}

fn header(base: u32, count: usize) -> Vec<u8> {
    let mut v = vec![0x01u8];
    v.extend_from_slice(&(1000 + base).to_le_bytes());
    v.extend_from_slice(&(count as u32).to_le_bytes());
    v
}

fn point(c: [f64; 3]) -> Vec<u8> {
    let mut v = vec![0x01u8];
    v.extend_from_slice(&1001u32.to_le_bytes());
    for o in c {
        v.extend_from_slice(&o.to_le_bytes());
    }
    v
}

fn run(base: u32, cs: &[[f64; 3]]) -> Vec<u8> {
    let mut v = header(base, cs.len());
    for c in cs {
        for o in c {
            v.extend_from_slice(&o.to_le_bytes());
        }
    }
    v
}

fn rings(base: u32, body: &str) -> Option<Vec<u8>> {
    let parts = split_parts(&body[1..body.len() - 1])?;
    let mut v = header(base, parts.len());
    for p in parts {
        let cs = coords(&p)?;
        v.extend_from_slice(&(cs.len() as u32).to_le_bytes());
        for c in cs {
            for o in c {
                v.extend_from_slice(&o.to_le_bytes());
            }
        }
    }
    Some(v)
}

/// Split `(a)(,)(b)` style sibling groups at depth 0.
fn split_parts(inner: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    for (i, ch) in inner.char_indices() {
        match ch {
            '(' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    out.push(inner[start?..=i].to_string());
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        // A flat list: `1 2 3,4 5 6`, as MULTIPOINT sometimes writes.
        out = inner
            .split(',')
            .map(|s| format!("({})", s.trim()))
            .collect();
    }
    Some(out)
}

fn coords(body: &str) -> Option<Vec<[f64; 3]>> {
    let inner = body.trim().trim_start_matches('(').trim_end_matches(')');
    inner.split(',').map(nums_str).collect()
}

fn nums(body: &str) -> Option<[f64; 3]> {
    nums_str(body.trim().trim_start_matches('(').trim_end_matches(')'))
}

fn nums_str(s: &str) -> Option<[f64; 3]> {
    let mut it = s.split_whitespace().map(|n| n.parse::<f64>());
    Some([
        it.next()?.ok()?,
        it.next()?.ok()?,
        it.next().transpose().ok()?.unwrap_or(0.0),
    ])
}
