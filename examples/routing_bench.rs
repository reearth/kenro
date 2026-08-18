//! Routing benchmark over a real OpenStreetMap road network.
//!
//! ```text
//! cargo run --release --features rusqlite,routing --example routing_bench -- \
//!     scripts/bench/routing/data/monaco.edges.csv \
//!     scripts/bench/routing/data/monaco.nodes.csv \
//!     [--pairs 100] [--seed 0x2545F4914F6CDD1D] [--bbox-margin-km 5] [--vs-pgrouting]
//! ```
//!
//! The CSVs come from `scripts/bench/routing/prepare.py`; see that
//! directory's README for the whole workflow.
//!
//! What this is measuring: kenro's routing aggregates accumulate every edge
//! row the query feeds them before searching, so a full-table query is O(E)
//! no matter how short the route is. The documented mitigation is a
//! `WHERE`-clause prefilter — this bench quantifies both the speedup and its
//! cost in wrong answers, since a route that leaves the prefilter box is a
//! route the aggregate can no longer find.
//!
//! No new dependencies: the CSV reader is a `split(',')`, the PRNG is an
//! inline xorshift with a fixed seed, and the report is `println!`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use rusqlite::{Connection, params};

// ---------------------------------------------------------------- options

struct Opts {
    edges: PathBuf,
    nodes: PathBuf,
    pairs: usize,
    seed: u64,
    margin_km: f64,
    vs_pgrouting: bool,
}

const USAGE: &str = "usage: routing_bench <edges.csv> <nodes.csv> \
[--pairs N] [--seed S] [--bbox-margin-km K] [--vs-pgrouting]";

fn parse_args() -> Opts {
    let mut positional: Vec<String> = Vec::new();
    let mut opts = Opts {
        edges: PathBuf::new(),
        nodes: PathBuf::new(),
        pairs: 100,
        seed: 0x2545_F491_4F6C_DD1D,
        margin_km: 5.0,
        vs_pgrouting: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .unwrap_or_else(|| panic!("{name} needs a value\n{USAGE}"))
        };
        match a.as_str() {
            "--pairs" => opts.pairs = value("--pairs").parse().expect("--pairs: integer"),
            "--seed" => {
                let s = value("--seed");
                opts.seed = s
                    .strip_prefix("0x")
                    .map(|h| u64::from_str_radix(h, 16))
                    .unwrap_or_else(|| s.parse())
                    .expect("--seed: integer or 0x-prefixed hex");
            }
            "--bbox-margin-km" => {
                opts.margin_km = value("--bbox-margin-km")
                    .parse()
                    .expect("--bbox-margin-km: number")
            }
            "--vs-pgrouting" => opts.vs_pgrouting = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => positional.push(other.to_string()),
        }
    }
    if positional.len() != 2 {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    opts.edges = PathBuf::from(&positional[0]);
    opts.nodes = PathBuf::from(&positional[1]);
    opts
}

// ------------------------------------------------------------------- prng

/// xorshift64*, seeded from `--seed` so a run is reproducible.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

// -------------------------------------------------------------- stats

struct Stats {
    median: f64,
    p95: f64,
    mean: f64,
}

fn stats(mut ms: Vec<f64>) -> Stats {
    ms.sort_by(f64::total_cmp);
    let n = ms.len();
    if n == 0 {
        return Stats {
            median: 0.0,
            p95: 0.0,
            mean: 0.0,
        };
    }
    let pick = |q: f64| ms[(((n as f64) * q) as usize).min(n - 1)];
    Stats {
        median: pick(0.5),
        p95: pick(0.95),
        mean: ms.iter().sum::<f64>() / n as f64,
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

// -------------------------------------------------------------- loading

struct Node {
    lon: f64,
    lat: f64,
}

fn load(conn: &Connection, edges_csv: &Path, nodes_csv: &Path) -> (usize, Vec<(i64, Node)>) {
    conn.execute_batch(
        "PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
         DROP TABLE IF EXISTS edges; DROP TABLE IF EXISTS nodes;
         CREATE TABLE edges(id INTEGER PRIMARY KEY, source INTEGER, target INTEGER,
                            cost REAL, reverse_cost REAL,
                            x1 REAL, y1 REAL, x2 REAL, y2 REAL);
         CREATE TABLE nodes(node INTEGER PRIMARY KEY, lon REAL, lat REAL);",
    )
    .expect("schema");

    let text = std::fs::read_to_string(edges_csv)
        .unwrap_or_else(|e| panic!("{}: {e}", edges_csv.display()));
    let mut edge_count = 0usize;
    {
        let tx = conn.unchecked_transaction().expect("tx");
        let mut ins = tx
            .prepare("INSERT INTO edges VALUES (?,?,?,?,?,?,?,?,?)")
            .expect("prepare");
        for line in text.lines().skip(1) {
            if line.is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split(',').collect();
            assert!(f.len() == 9, "edges.csv: expected 9 columns, got {line}");
            let num = |i: usize| f[i].parse::<f64>().expect("edges.csv: number");
            ins.execute(params![
                f[0].parse::<i64>().expect("edges.csv: id"),
                f[1].parse::<i64>().expect("edges.csv: source"),
                f[2].parse::<i64>().expect("edges.csv: target"),
                num(3),
                num(4),
                num(5),
                num(6),
                num(7),
                num(8),
            ])
            .expect("insert edge");
            edge_count += 1;
        }
        drop(ins);
        tx.commit().expect("commit edges");
    }
    drop(text);

    let text = std::fs::read_to_string(nodes_csv)
        .unwrap_or_else(|e| panic!("{}: {e}", nodes_csv.display()));
    let mut nodes = Vec::new();
    {
        let tx = conn.unchecked_transaction().expect("tx");
        let mut ins = tx
            .prepare("INSERT INTO nodes VALUES (?,?,?)")
            .expect("prepare");
        for line in text.lines().skip(1) {
            if line.is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split(',').collect();
            assert!(f.len() == 3, "nodes.csv: expected 3 columns, got {line}");
            let node = f[0].parse::<i64>().expect("nodes.csv: node");
            let lon = f[1].parse::<f64>().expect("nodes.csv: lon");
            let lat = f[2].parse::<f64>().expect("nodes.csv: lat");
            ins.execute(params![node, lon, lat]).expect("insert node");
            nodes.push((node, Node { lon, lat }));
        }
        drop(ins);
        tx.commit().expect("commit nodes");
    }

    conn.execute_batch(
        "CREATE INDEX edges_bbox_x ON edges(x1, x2, y1, y2);
         CREATE INDEX edges_bbox_y ON edges(y1, y2, x1, x2);",
    )
    .expect("index");
    (edge_count, nodes)
}

// -------------------------------------------------------------- the bench

/// The prefilter envelope: the two endpoints' bounding box, grown by
/// `margin_km`. Degrees are converted with the flat approximation
/// 1 deg latitude = 111 km and 1 deg longitude = 111 km * cos(lat) — good
/// enough for a margin (it is deliberately generous, not a projection).
fn envelope(a: &Node, b: &Node, margin_km: f64) -> (f64, f64, f64, f64) {
    let dlat = margin_km / 111.0;
    let mid = ((a.lat + b.lat) / 2.0).to_radians();
    let dlon = dlat / mid.cos().abs().max(0.05);
    (
        a.lon.min(b.lon) - dlon,
        a.lat.min(b.lat) - dlat,
        a.lon.max(b.lon) + dlon,
        a.lat.max(b.lat) + dlat,
    )
}

const SQL_COST_FULL: &str =
    "SELECT kenro_dijkstra_cost(source, target, cost, ?1, ?2, reverse_cost) FROM edges";
const SQL_PATH_FULL: &str =
    "SELECT kenro_dijkstra(id, source, target, cost, ?1, ?2, reverse_cost) FROM edges";
const SQL_COST_BBOX: &str = "SELECT kenro_dijkstra_cost(source, target, cost, ?1, ?2, reverse_cost)
     FROM edges WHERE x1 <= ?4 AND x2 >= ?3 AND y1 <= ?6 AND y2 >= ?5";
const SQL_COUNT_BBOX: &str =
    "SELECT COUNT(*) FROM edges WHERE x1 <= ?2 AND x2 >= ?1 AND y1 <= ?4 AND y2 >= ?3";

fn table(name: &str, s: &Stats, extra: &str) -> String {
    format!(
        "| {name} | {:.1} | {:.1} | {:.1} | {extra} |",
        s.median, s.p95, s.mean
    )
}

fn main() {
    let opts = parse_args();
    let db_path = opts
        .edges
        .parent()
        .unwrap_or(Path::new("."))
        .join("bench.sqlite");
    let _ = std::fs::remove_file(&db_path);
    let conn = Connection::open(&db_path).expect("open db");
    kenro::register(&conn).expect("register kenro");

    let t0 = Instant::now();
    let (edge_count, nodes) = load(&conn, &opts.edges, &opts.nodes);
    let load_ms = ms(t0.elapsed());
    assert!(!nodes.is_empty(), "nodes.csv is empty");

    let dataset = opts
        .edges
        .file_name()
        .map(|s| s.to_string_lossy().replace(".edges.csv", ""))
        .unwrap_or_default();

    // Random pairs, seeded: the same seed picks the same pairs on any host.
    let mut rng = Rng(opts.seed | 1);
    let pairs: Vec<(usize, usize)> = (0..opts.pairs)
        .map(|_| (rng.below(nodes.len()), rng.below(nodes.len())))
        .collect();

    // --- (a) kenro_dijkstra_cost, full table ---
    let mut full_ms = Vec::new();
    let mut full_cost: Vec<Option<f64>> = Vec::new();
    for &(i, j) in &pairs {
        let t = Instant::now();
        let c: Option<f64> = conn
            .query_row(SQL_COST_FULL, params![nodes[i].0, nodes[j].0], |r| r.get(0))
            .expect("cost full");
        full_ms.push(ms(t.elapsed()));
        full_cost.push(c);
    }
    let full_unreachable = full_cost.iter().filter(|c| c.is_none()).count();

    // --- (b) kenro_dijkstra, full table (JSON path) ---
    let mut path_ms = Vec::new();
    let mut path_steps = 0usize;
    let mut path_unreachable = 0usize;
    for &(i, j) in &pairs {
        let t = Instant::now();
        let p: Option<String> = conn
            .query_row(SQL_PATH_FULL, params![nodes[i].0, nodes[j].0], |r| r.get(0))
            .expect("path full");
        path_ms.push(ms(t.elapsed()));
        match p {
            None => path_unreachable += 1,
            // One step per `{`; no JSON parser needed for a row count.
            Some(j) => path_steps += j.matches("{\"").count(),
        }
    }

    // --- (c) kenro_dijkstra_cost with the bbox prefilter ---
    let mut bbox_ms = Vec::new();
    let mut bbox_unreachable = 0usize;
    let mut mismatches = 0usize;
    let mut fed_total = 0u64;
    for (k, &(i, j)) in pairs.iter().enumerate() {
        let (minx, miny, maxx, maxy) = envelope(&nodes[i].1, &nodes[j].1, opts.margin_km);
        let fed: i64 = conn
            .query_row(SQL_COUNT_BBOX, params![minx, maxx, miny, maxy], |r| {
                r.get(0)
            })
            .expect("count bbox");
        fed_total += fed as u64;
        let t = Instant::now();
        let c: Option<f64> = conn
            .query_row(
                SQL_COST_BBOX,
                params![nodes[i].0, nodes[j].0, minx, maxx, miny, maxy],
                |r| r.get(0),
            )
            .expect("cost bbox");
        bbox_ms.push(ms(t.elapsed()));
        if c.is_none() {
            bbox_unreachable += 1;
        }
        if !same_cost(c, full_cost[k]) {
            mismatches += 1;
        }
    }

    // --- (d) kenro_drivingdistance ---
    let dd_starts: Vec<i64> = (0..5.min(nodes.len()))
        .map(|_| nodes[rng.below(nodes.len())].0)
        .collect();
    let dd_limits = [1000.0_f64, 5000.0];
    let mut dd_rows = Vec::new();
    for &limit in &dd_limits {
        let mut times = Vec::new();
        let mut reached = Vec::new();
        for &s in &dd_starts {
            let t = Instant::now();
            let r: Option<String> = conn
                .query_row(
                    "SELECT kenro_drivingdistance(id, source, target, cost, ?1, ?2, reverse_cost)
                     FROM edges",
                    params![s, limit],
                    |r| r.get(0),
                )
                .expect("drivingdistance");
            times.push(ms(t.elapsed()));
            reached.push(r.map(|j| j.matches("{\"").count()).unwrap_or(0));
        }
        let avg_reached = reached.iter().sum::<usize>() as f64 / reached.len() as f64;
        dd_rows.push((limit, stats(times), avg_reached));
    }

    // ------------------------------------------------------------- report
    let full = stats(full_ms);
    let path = stats(path_ms);
    let bbox = stats(bbox_ms);
    let n = pairs.len();
    let avg_fed = fed_total as f64 / n as f64;

    println!("# kenro routing bench — {dataset}\n");
    println!("| dataset | edges | vertices | pairs | seed | bbox margin |");
    println!("|---|---|---|---|---|---|");
    println!(
        "| {dataset} | {edge_count} | {} | {n} | 0x{:016X} | {} km |",
        nodes.len(),
        opts.seed,
        opts.margin_km
    );
    println!("\nCSV load into SQLite: {load_ms:.0} ms.\n");

    println!("## Shortest path, one pair per query\n");
    println!("| variant | median ms | p95 ms | mean ms | notes |");
    println!("|---|---|---|---|---|");
    println!(
        "{}",
        table(
            "(a) `kenro_dijkstra_cost`, full table",
            &full,
            &format!("{full_unreachable}/{n} unreachable, {edge_count} edges fed")
        )
    );
    println!(
        "{}",
        table(
            "(b) `kenro_dijkstra`, full table (JSON path)",
            &path,
            &format!(
                "{path_unreachable}/{n} unreachable, {:.0} steps/path avg",
                path_steps as f64 / (n - path_unreachable).max(1) as f64
            )
        )
    );
    println!(
        "{}",
        table(
            "(c) `kenro_dijkstra_cost`, bbox prefilter",
            &bbox,
            &format!(
                "{bbox_unreachable}/{n} unreachable, {avg_fed:.0} edges fed avg ({:.1}% of table), \
                 **{mismatches}/{n} answers differ from (a)**",
                100.0 * avg_fed / edge_count.max(1) as f64
            )
        )
    );
    println!(
        "\nThe prefilter feeds {:.1}x fewer edges and runs at {:.2}x the median latency of (a) \
         (below 1.00 is faster). Its honest cost is the {mismatches} pair(s) out of {n} whose \
         answer changed — a route that leaves the box is a route the aggregate can no longer \
         find. A prefilter that still selects most of the table buys nothing and can lose to \
         (a) on the `WHERE` clause alone.",
        edge_count as f64 / avg_fed.max(1.0),
        bbox.median / full.median.max(f64::MIN_POSITIVE)
    );

    println!("\n## `kenro_drivingdistance`, full table\n");
    println!("| limit | median ms | p95 ms | mean ms | avg nodes reached |");
    println!("|---|---|---|---|---|");
    for (limit, s, reached) in &dd_rows {
        println!(
            "| {limit:.0} m | {:.1} | {:.1} | {:.1} | {reached:.0} |",
            s.median, s.p95, s.mean
        );
    }
    println!(
        "\n{} random starts per limit; the start's own row is included in the count.",
        dd_starts.len()
    );

    if opts.vs_pgrouting {
        let pairs_vids: Vec<(i64, i64)> = pairs
            .iter()
            .map(|&(i, j)| (nodes[i].0, nodes[j].0))
            .collect();
        pgrouting(&opts, &pairs_vids, &full_cost, &full);
    }
}

/// Costs agree when both are NULL, or when they match within 1e-6 relative.
fn same_cost(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => (x - y).abs() <= 1e-6 * x.abs().max(y.abs()).max(1.0),
        _ => false,
    }
}

// --------------------------------------------------------- pgRouting side

const IMAGE: &str = "pgrouting/pgrouting:17-3.5-3.7";
const CONTAINER: &str = "kenro-bench-pgrouting";

fn sh(cmd: &str) -> std::process::Output {
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .unwrap_or_else(|e| panic!("running `{cmd}`: {e}"))
}

fn psql(sql: &str) -> String {
    let out = sh(&format!(
        "docker exec -i {CONTAINER} psql -U postgres -tA -v ON_ERROR_STOP=1 <<'KENRO_SQL'\n{sql}\nKENRO_SQL"
    ));
    if !out.status.success() {
        panic!("psql failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn pgrouting(opts: &Opts, pairs: &[(i64, i64)], kenro_cost: &[Option<f64>], kenro_stats: &Stats) {
    eprintln!("starting {IMAGE}...");
    let _ = sh(&format!("docker rm -f {CONTAINER} >/dev/null 2>&1"));
    // --platform: the reference image is amd64-only and runs under emulation
    // on arm64 hosts, which is exactly why the timing caveat below exists.
    let run = sh(&format!(
        "docker run --rm -d --name {CONTAINER} --platform linux/amd64 \
         -e POSTGRES_PASSWORD=kenro {IMAGE}"
    ));
    assert!(
        run.status.success(),
        "docker run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    // Same wait-for-init pattern as scripts/golden/routing_generate.sh.
    let ready = sh(&format!(
        "until docker logs {CONTAINER} 2>&1 | grep -q 'PostgreSQL init process complete'; \
         do sleep 1; done; \
         until docker exec {CONTAINER} psql -U postgres -tAc 'SELECT 1' >/dev/null 2>&1; \
         do sleep 1; done"
    ));
    assert!(ready.status.success(), "postgres never came up");

    psql(
        "CREATE EXTENSION IF NOT EXISTS postgis;
         CREATE EXTENSION IF NOT EXISTS pgrouting;
         DROP TABLE IF EXISTS edges;
         CREATE TABLE edges(id BIGINT PRIMARY KEY, source BIGINT, target BIGINT,
                            cost DOUBLE PRECISION, reverse_cost DOUBLE PRECISION,
                            x1 DOUBLE PRECISION, y1 DOUBLE PRECISION,
                            x2 DOUBLE PRECISION, y2 DOUBLE PRECISION);",
    );
    let copy = sh(&format!(
        "docker exec -i {CONTAINER} psql -U postgres -q -v ON_ERROR_STOP=1 \
         -c \"\\copy edges FROM STDIN WITH (FORMAT csv, HEADER true)\" < '{}'",
        opts.edges.display()
    ));
    assert!(
        copy.status.success(),
        "\\copy failed: {}",
        String::from_utf8_lossy(&copy.stderr)
    );
    // The indexes osm2pgrouting/pgr_createTopology create.
    psql(
        "CREATE INDEX edges_source_idx ON edges(source);
         CREATE INDEX edges_target_idx ON edges(target);
         ANALYZE edges;",
    );
    let version = psql("SELECT pgr_version()").trim().to_string();

    let mut sql = String::from("\\timing on\n");
    for &(s, e) in pairs {
        sql.push_str(&format!(
            "SELECT coalesce(max(agg_cost)::text, 'NULL') FROM pgr_dijkstraCost(\
             'SELECT id, source, target, cost, reverse_cost FROM edges', {s}, {e});\n"
        ));
    }
    eprintln!("running {} pgr_dijkstraCost queries...", pairs.len());
    let t = Instant::now();
    let out = psql(&sql);
    let wall = ms(t.elapsed());

    let mut costs: Vec<Option<f64>> = Vec::new();
    let mut times: Vec<f64> = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Time:") {
            let v = rest.trim().trim_end_matches(" ms");
            if let Ok(t) = v.parse::<f64>() {
                times.push(t);
            }
        } else if line == "NULL" {
            costs.push(None);
        } else if let Ok(v) = line.parse::<f64>() {
            costs.push(Some(v));
        }
    }
    assert_eq!(costs.len(), pairs.len(), "unexpected psql output:\n{out}");

    let agree = costs
        .iter()
        .zip(kenro_cost)
        .filter(|(a, b)| same_cost(**a, **b))
        .count();
    let both_null = costs
        .iter()
        .zip(kenro_cost)
        .filter(|(a, b)| a.is_none() && b.is_none())
        .count();
    let pg = stats(times);

    println!("\n## vs pgRouting ({version})\n");
    println!("| | value |");
    println!("|---|---|");
    println!(
        "| agg_cost agreement (1e-6 relative) | **{agree}/{}** ({both_null} of them both-NULL) |",
        pairs.len()
    );
    println!(
        "| `pgr_dijkstraCost` server-side | median {:.1} ms, p95 {:.1} ms, mean {:.1} ms |",
        pg.median, pg.p95, pg.mean
    );
    println!(
        "| whole batch through `docker exec psql` | {wall:.0} ms for {} queries |",
        pairs.len()
    );
    println!(
        "| kenro `kenro_dijkstra_cost` full table | median {:.1} ms, p95 {:.1} ms, mean {:.1} ms |",
        kenro_stats.median, kenro_stats.p95, kenro_stats.mean
    );
    println!(
        "\n> Timing caveat: the two sides are not measured the same way. kenro is timed \
         in-process around `query_row`, pgRouting by its own `\\timing` (server-side, \
         excluding client and Docker overhead) inside a batch driven through \
         `docker exec`, in an amd64 container that is emulated on arm64 hosts. Read the \
         agreement number as exact and the latency comparison as an order of magnitude."
    );

    let _ = sh(&format!("docker stop {CONTAINER} >/dev/null 2>&1"));
}
