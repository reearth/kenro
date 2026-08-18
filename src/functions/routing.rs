//! pgRouting-style shortest paths as aggregate functions: the `WHERE`
//! clause plays the role of pgRouting's SQL-string argument, and the
//! accumulator materializes the edge set before searching — exactly what
//! pgRouting itself does with the rows its query returns.
//!
//! `kenro_dijkstra(id, source, target, cost, start_vid, end_vid
//! [, reverse_cost])` and its `kenro_dijkstra_cost` /
//! `kenro_drivingdistance` siblings deliberately diverge from
//! `pgr_dijkstra`'s column
//! order: `reverse_cost` is trailing because every kenro host treats
//! trailing arguments as the optional ones (the same accommodation
//! `ST_AsMVT` makes for the missing record type). Semantics follow
//! pgRouting: the graph is directed, a negative `cost` makes the edge
//! impassable source→target, and `reverse_cost` — negative likewise — is
//! the cost of traversing target→source. Node and edge ids are 32-bit;
//! kenro stays off 64-bit integers outside the H3 family so every host
//! can carry them.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};

use crate::error::{Error, Result};

#[derive(Clone, Copy)]
struct Edge {
    id: i32,
    source: i32,
    target: i32,
    cost: f64,
    reverse_cost: Option<f64>,
}

/// One row of the `pgr_dijkstra`-shaped result: the node reached, the edge
/// leaving it (`-1` on the terminal row), that edge's cost, and the running
/// total on arrival at the node.
struct PathStep {
    node: i32,
    edge: i64,
    cost: f64,
    agg_cost: f64,
}

fn check_finite(func: &'static str, name: &str, v: f64) -> Result<()> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(Error::Unsupported {
            func,
            reason: format!("{name} must be finite, got {v}"),
        })
    }
}

/// Aggregate accumulator for `kenro_dijkstra`.
pub struct DijkstraAggregate {
    edges: Vec<Edge>,
    endpoints: Option<(i32, i32)>,
}

impl DijkstraAggregate {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        DijkstraAggregate {
            edges: Vec::new(),
            endpoints: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn step_impl(
        &mut self,
        func: &'static str,
        id: i32,
        source: i32,
        target: i32,
        cost: f64,
        start_vid: i32,
        end_vid: i32,
        reverse_cost: Option<f64>,
    ) -> Result<()> {
        check_finite(func, "cost", cost)?;
        if let Some(rc) = reverse_cost {
            check_finite(func, "reverse_cost", rc)?;
        }
        match self.endpoints {
            None => self.endpoints = Some((start_vid, end_vid)),
            Some(existing) => {
                if existing != (start_vid, end_vid) {
                    return Err(Error::Unsupported {
                        func,
                        reason: "start_vid and end_vid must be constant within one \
                                 aggregation group"
                            .into(),
                    });
                }
            }
        }
        self.edges.push(Edge {
            id,
            source,
            target,
            cost,
            reverse_cost,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        id: i32,
        source: i32,
        target: i32,
        cost: f64,
        start_vid: i32,
        end_vid: i32,
        reverse_cost: Option<f64>,
    ) -> Result<()> {
        self.step_impl(
            "kenro_dijkstra",
            id,
            source,
            target,
            cost,
            start_vid,
            end_vid,
            reverse_cost,
        )
    }

    /// `None` = SQL NULL: zero input rows, no path, or an endpoint absent
    /// from the edge set (`pgr_dijkstra` returns the empty set for all of
    /// these).
    pub fn finish(self) -> Result<Option<String>> {
        let Some((start, end)) = self.endpoints else {
            return Ok(None);
        };
        Ok(shortest_path(&self.edges, start, end).map(|steps| path_json(&steps)))
    }
}

/// Aggregate accumulator for `kenro_dijkstra_cost` — the same search
/// without the edge-id column, returning only the total cost.
pub struct DijkstraCostAggregate {
    inner: DijkstraAggregate,
}

impl DijkstraCostAggregate {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        DijkstraCostAggregate {
            inner: DijkstraAggregate::new(),
        }
    }

    pub fn step(
        &mut self,
        source: i32,
        target: i32,
        cost: f64,
        start_vid: i32,
        end_vid: i32,
        reverse_cost: Option<f64>,
    ) -> Result<()> {
        // No id column in this signature; the ids never surface in a cost.
        self.inner.step_impl(
            "kenro_dijkstra_cost",
            0,
            source,
            target,
            cost,
            start_vid,
            end_vid,
            reverse_cost,
        )
    }

    /// `None` = SQL NULL: zero input rows or the target is unreachable.
    pub fn finish(self) -> Result<Option<f64>> {
        let Some((start, end)) = self.inner.endpoints else {
            return Ok(None);
        };
        Ok(shortest_path(&self.inner.edges, start, end)
            .and_then(|steps| steps.last().map(|s| s.agg_cost)))
    }
}

/// Aggregate accumulator for `kenro_drivingdistance` — every node reachable
/// from `start_vid` within `limit`, rather than one route to one node.
pub struct DrivingDistanceAggregate {
    edges: Vec<Edge>,
    params: Option<(i32, f64)>,
}

impl DrivingDistanceAggregate {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        DrivingDistanceAggregate {
            edges: Vec::new(),
            params: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        id: i32,
        source: i32,
        target: i32,
        cost: f64,
        start_vid: i32,
        limit: f64,
        reverse_cost: Option<f64>,
    ) -> Result<()> {
        const FUNC: &str = "kenro_drivingdistance";
        check_finite(FUNC, "cost", cost)?;
        check_finite(FUNC, "limit", limit)?;
        if let Some(rc) = reverse_cost {
            check_finite(FUNC, "reverse_cost", rc)?;
        }
        match self.params {
            None => self.params = Some((start_vid, limit)),
            Some(existing) => {
                if existing.0 != start_vid || existing.1.total_cmp(&limit) != Ordering::Equal {
                    return Err(Error::Unsupported {
                        func: FUNC,
                        reason: "start_vid and limit must be constant within one \
                                 aggregation group"
                            .into(),
                    });
                }
            }
        }
        self.edges.push(Edge {
            id,
            source,
            target,
            cost,
            reverse_cost,
        });
        Ok(())
    }

    /// `None` = SQL NULL: zero input rows, or a negative `limit` — both of
    /// which make `pgr_drivingDistance` return the empty set. Note what is
    /// *not* on that list: a `start_vid` absent from the edge set still
    /// answers with its own row, because a vertex is always within any
    /// non-negative distance of itself. That is measured pgRouting
    /// behaviour, not a choice.
    pub fn finish(self) -> Result<Option<String>> {
        let Some((start, limit)) = self.params else {
            return Ok(None);
        };
        Ok(driving_distance(&self.edges, start, limit).map(|rows| reach_json(&rows)))
    }
}

/// One row of the `pgr_drivingDistance`-shaped result.
struct ReachStep {
    depth: u32,
    pred: i32,
    node: i32,
    edge: i64,
    cost: f64,
    agg_cost: f64,
}

/// Every node whose shortest-path cost from `start` is at most `limit`.
/// `None` only for a negative `limit`; an unknown `start` still yields its
/// own row, as `pgr_drivingDistance` does.
fn driving_distance(edges: &[Edge], start: i32, limit: f64) -> Option<Vec<ReachStep>> {
    if limit < 0.0 {
        return None;
    }
    // The start node's own row, which is present whether or not any edge
    // mentions it.
    let mut rows = vec![ReachStep {
        depth: 0,
        pred: start,
        node: start,
        edge: -1,
        cost: 0.0,
        agg_cost: 0.0,
    }];
    let g = Graph::build(edges);
    if let Some(&start_ix) = g.index.get(&start) {
        // No early exit: the sweep has to settle everything within reach.
        let r = g.dijkstra(start_ix, None);
        for ix in 0..g.by_index.len() {
            if ix == start_ix || !r.settled[ix] || r.dist[ix] > limit {
                continue;
            }
            let (from, id, cost) = r.prev[ix].expect("a settled node has a trail");
            rows.push(ReachStep {
                depth: r.depth[ix],
                pred: g.by_index[from],
                node: g.by_index[ix],
                edge: id as i64,
                cost,
                agg_cost: r.dist[ix],
            });
        }
    }
    // pgRouting emits these in the order its own tree traversal happens to
    // reach them, which is not a contract. Nearest-first, ties by node id,
    // is at least an order a caller can rely on.
    rows.sort_by(|a, b| a.agg_cost.total_cmp(&b.agg_cost).then(a.node.cmp(&b.node)));
    Some(rows)
}

fn reach_json(rows: &[ReachStep]) -> String {
    let rows: Vec<serde_json::Value> = rows
        .iter()
        .enumerate()
        .map(|(i, s)| {
            serde_json::json!({
                "seq": i + 1,
                "depth": s.depth,
                "pred": s.pred,
                "node": s.node,
                "edge": s.edge,
                "cost": s.cost,
                "agg_cost": s.agg_cost,
            })
        })
        .collect();
    serde_json::Value::Array(rows).to_string()
}

fn path_json(steps: &[PathStep]) -> String {
    let rows: Vec<serde_json::Value> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            serde_json::json!({
                "seq": i + 1,
                "node": s.node,
                "edge": s.edge,
                "cost": s.cost,
                "agg_cost": s.agg_cost,
            })
        })
        .collect();
    serde_json::Value::Array(rows).to_string()
}

/// The edge set as an adjacency list over densely-numbered nodes. One
/// direction per traversable orientation: `cost >= 0` gives source→target,
/// `reverse_cost >= 0` gives target→source, and either being negative simply
/// omits that arc — which is how pgRouting spells a one-way street.
struct Graph {
    /// node id → dense index. A `BTreeMap` so the numbering, and therefore
    /// the heap's tie-break, is deterministic whatever order the rows
    /// arrived in.
    index: BTreeMap<i32, usize>,
    by_index: Vec<i32>,
    adj: Vec<Vec<(usize, f64, i32)>>,
}

/// What Dijkstra learned about one node: its distance, whether it was
/// reached at all, and the arc it was reached by.
struct Reached {
    dist: Vec<f64>,
    settled: Vec<bool>,
    /// (previous node, edge id taken, that edge's cost)
    prev: Vec<Option<(usize, i32, f64)>>,
    depth: Vec<u32>,
}

impl Graph {
    fn build(edges: &[Edge]) -> Graph {
        let mut index: BTreeMap<i32, usize> = BTreeMap::new();
        for e in edges {
            let n = index.len();
            index.entry(e.source).or_insert(n);
            let n = index.len();
            index.entry(e.target).or_insert(n);
        }
        let mut adj: Vec<Vec<(usize, f64, i32)>> = vec![Vec::new(); index.len()];
        for e in edges {
            let s = index[&e.source];
            let t = index[&e.target];
            if e.cost >= 0.0 {
                adj[s].push((t, e.cost, e.id));
            }
            if let Some(rc) = e.reverse_cost
                && rc >= 0.0
            {
                adj[t].push((s, rc, e.id));
            }
        }
        let mut by_index = vec![0; index.len()];
        for (&node, &ix) in &index {
            by_index[ix] = node;
        }
        Graph {
            index,
            by_index,
            adj,
        }
    }

    /// Dijkstra from `start_ix`. `stop_at` ends the search as soon as that
    /// node is settled — an optimization for a point-to-point query, and
    /// exactly what a driving-distance sweep must not do.
    fn dijkstra(&self, start_ix: usize, stop_at: Option<usize>) -> Reached {
        struct Item {
            dist: f64,
            node: usize,
        }
        impl PartialEq for Item {
            fn eq(&self, o: &Self) -> bool {
                self.cmp(o) == Ordering::Equal
            }
        }
        impl Eq for Item {}
        impl PartialOrd for Item {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for Item {
            // Reversed for a min-heap; ties broken on node index so the
            // exploration order is deterministic.
            fn cmp(&self, o: &Self) -> Ordering {
                o.dist
                    .total_cmp(&self.dist)
                    .then_with(|| o.node.cmp(&self.node))
            }
        }

        let n = self.index.len();
        let mut r = Reached {
            dist: vec![f64::INFINITY; n],
            settled: vec![false; n],
            prev: vec![None; n],
            depth: vec![0; n],
        };
        let mut heap = BinaryHeap::new();
        r.dist[start_ix] = 0.0;
        heap.push(Item {
            dist: 0.0,
            node: start_ix,
        });
        while let Some(Item { dist: d, node }) = heap.pop() {
            if r.settled[node] {
                continue;
            }
            r.settled[node] = true;
            if Some(node) == stop_at {
                break;
            }
            for &(to, cost, id) in &self.adj[node] {
                let nd = d + cost;
                if nd < r.dist[to] {
                    r.dist[to] = nd;
                    r.prev[to] = Some((node, id, cost));
                    r.depth[to] = r.depth[node] + 1;
                    heap.push(Item { dist: nd, node: to });
                }
            }
        }
        r
    }
}

/// Textbook Dijkstra with a predecessor trail. `None` when `start` or `end`
/// does not appear in the edge set, or no path exists. `start == end`
/// returns `None` too, matching `pgr_dijkstra`'s empty set.
fn shortest_path(edges: &[Edge], start: i32, end: i32) -> Option<Vec<PathStep>> {
    if start == end {
        return None;
    }
    let g = Graph::build(edges);
    let start_ix = *g.index.get(&start)?;
    let end_ix = *g.index.get(&end)?;
    let r = g.dijkstra(start_ix, Some(end_ix));
    if !r.settled[end_ix] {
        return None;
    }

    // Walk the trail back, then emit rows front-to-back: each row is the
    // node reached and the edge leaving it, the terminal row carries -1.
    let mut hops = Vec::new(); // (from index, edge id, edge cost)
    let mut at = end_ix;
    while at != start_ix {
        let (from, id, cost) = r.prev[at].expect("settled node has a trail");
        hops.push((from, id, cost));
        at = from;
    }
    hops.reverse();
    let mut steps = Vec::with_capacity(hops.len() + 1);
    let mut agg = 0.0;
    for (from, id, cost) in hops {
        steps.push(PathStep {
            node: g.by_index[from],
            edge: id as i64,
            cost,
            agg_cost: agg,
        });
        agg += cost;
    }
    steps.push(PathStep {
        node: end,
        edge: -1,
        cost: 0.0,
        agg_cost: agg,
    });
    Some(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dijkstra(
        rows: &[(i32, i32, i32, f64, Option<f64>)],
        start: i32,
        end: i32,
    ) -> Option<String> {
        let mut acc = DijkstraAggregate::new();
        for &(id, s, t, c, rc) in rows {
            acc.step(id, s, t, c, start, end, rc).unwrap();
        }
        acc.finish().unwrap()
    }

    fn cost(rows: &[(i32, i32, i32, f64, Option<f64>)], start: i32, end: i32) -> Option<f64> {
        let mut acc = DijkstraCostAggregate::new();
        for &(_, s, t, c, rc) in rows {
            acc.step(s, t, c, start, end, rc).unwrap();
        }
        acc.finish().unwrap()
    }

    fn nodes_of(json: &str) -> Vec<i64> {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        v.as_array()
            .unwrap()
            .iter()
            .map(|row| row["node"].as_i64().unwrap())
            .collect()
    }

    #[test]
    fn chain_path_and_rows() {
        let rows = [
            (10, 1, 2, 1.1, None),
            (11, 2, 3, 0.7, None),
            (12, 3, 4, 2.9, None),
        ];
        let json = dijkstra(&rows, 1, 4).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[0]["seq"], 1);
        assert_eq!(arr[0]["node"], 1);
        assert_eq!(arr[0]["edge"], 10);
        assert_eq!(arr[0]["agg_cost"], 0.0);
        assert_eq!(arr[3]["node"], 4);
        assert_eq!(arr[3]["edge"], -1);
        assert_eq!(arr[3]["cost"], 0.0);
        assert!((arr[3]["agg_cost"].as_f64().unwrap() - 4.7).abs() < 1e-12);
        assert_eq!(
            cost(&rows, 1, 4).unwrap(),
            arr[3]["agg_cost"].as_f64().unwrap()
        );
    }

    #[test]
    fn reverse_cost_is_the_backward_direction() {
        // 1→2 costs 1.0, 2→1 costs 5.0: the two directions differ.
        let rows = [(1, 1, 2, 1.0, Some(5.0))];
        assert_eq!(cost(&rows, 1, 2), Some(1.0));
        assert_eq!(cost(&rows, 2, 1), Some(5.0));
    }

    #[test]
    fn negative_cost_is_impassable_in_that_direction() {
        // Forward blocked, backward open.
        let rows = [(1, 1, 2, -1.0, Some(3.3))];
        assert_eq!(cost(&rows, 1, 2), None);
        assert_eq!(cost(&rows, 2, 1), Some(3.3));
    }

    #[test]
    fn unreachable_and_missing_vertices_are_null() {
        let rows = [(1, 1, 2, 1.0, None), (2, 3, 4, 1.0, None)];
        assert_eq!(dijkstra(&rows, 1, 4), None); // disconnected
        assert_eq!(dijkstra(&rows, 1, 99), None); // end not in the graph
        assert_eq!(dijkstra(&rows, 99, 4), None); // start not in the graph
        assert_eq!(dijkstra(&rows, 2, 1), None); // directed: no way back
    }

    #[test]
    fn start_equals_end_is_null() {
        let rows = [(1, 1, 2, 1.0, None)];
        assert_eq!(dijkstra(&rows, 1, 1), None);
        assert_eq!(cost(&rows, 1, 1), None);
    }

    #[test]
    fn zero_rows_are_null() {
        assert_eq!(DijkstraAggregate::new().finish().unwrap(), None);
        assert_eq!(DijkstraCostAggregate::new().finish().unwrap(), None);
    }

    #[test]
    fn parallel_edges_take_the_cheaper() {
        let rows = [(1, 1, 2, 9.0, None), (2, 1, 2, 2.0, None)];
        let json = dijkstra(&rows, 1, 2).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["edge"], 2);
        assert_eq!(cost(&rows, 1, 2), Some(2.0));
    }

    #[test]
    fn a_detour_beats_a_dear_direct_edge() {
        let rows = [
            (1, 1, 3, 10.0, None),
            (2, 1, 2, 1.5, None),
            (3, 2, 3, 1.5, None),
        ];
        let json = dijkstra(&rows, 1, 3).unwrap();
        assert_eq!(nodes_of(&json), vec![1, 2, 3]);
        assert_eq!(cost(&rows, 1, 3), Some(3.0));
    }

    #[test]
    fn self_loops_are_harmless() {
        let rows = [(1, 1, 1, 0.5, None), (2, 1, 2, 1.0, None)];
        assert_eq!(cost(&rows, 1, 2), Some(1.0));
    }

    #[test]
    fn changing_endpoints_mid_group_errors() {
        let mut acc = DijkstraAggregate::new();
        acc.step(1, 1, 2, 1.0, 1, 2, None).unwrap();
        let err = acc.step(2, 2, 3, 1.0, 1, 3, None).unwrap_err();
        assert!(err.to_string().contains("must be constant"), "{err}");
    }

    #[test]
    fn non_finite_costs_error() {
        let mut acc = DijkstraAggregate::new();
        let err = acc.step(1, 1, 2, f64::NAN, 1, 2, None).unwrap_err();
        assert!(err.to_string().contains("finite"), "{err}");
        let mut acc = DijkstraAggregate::new();
        let err = acc
            .step(1, 1, 2, 1.0, 1, 2, Some(f64::INFINITY))
            .unwrap_err();
        assert!(err.to_string().contains("finite"), "{err}");
    }

    fn driving(
        rows: &[(i32, i32, i32, f64, Option<f64>)],
        start: i32,
        limit: f64,
    ) -> Option<serde_json::Value> {
        let mut acc = DrivingDistanceAggregate::new();
        for &(id, s, t, c, rc) in rows {
            acc.step(id, s, t, c, start, limit, rc).unwrap();
        }
        acc.finish()
            .unwrap()
            .map(|j| serde_json::from_str(&j).unwrap())
    }

    #[test]
    fn driving_distance_stops_at_the_limit() {
        let rows = [
            (10, 1, 2, 1.1, None),
            (11, 2, 3, 0.7, None),
            (12, 3, 4, 2.9, None),
        ];
        // Inclusive: 1.8 is exactly node 3's cost.
        let v = driving(&rows, 1, 1.8).unwrap();
        let nodes: Vec<i64> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["node"].as_i64().unwrap())
            .collect();
        assert_eq!(nodes, vec![1, 2, 3]);
        assert_eq!(v[0]["edge"], -1);
        assert_eq!(v[0]["pred"], 1);
        assert_eq!(v[0]["depth"], 0);
        assert_eq!(v[2]["depth"], 2);
        assert_eq!(v[2]["pred"], 2);
        assert_eq!(v[2]["edge"], 11);
        // A hair under, and node 3 falls out.
        assert_eq!(
            driving(&rows, 1, 1.79).unwrap().as_array().unwrap().len(),
            2
        );
    }

    #[test]
    fn driving_distance_always_reaches_itself() {
        let rows = [(10, 1, 2, 1.1, None)];
        // A limit of zero, and a start the edge set never mentions, both
        // still answer with the start's own row (measured pgRouting).
        for (start, limit) in [(1, 0.0), (99, 5.0)] {
            let v = driving(&rows, start, limit).unwrap();
            assert_eq!(v.as_array().unwrap().len(), 1, "{start}/{limit}");
            assert_eq!(v[0]["node"], start);
            assert_eq!(v[0]["agg_cost"], 0.0);
        }
    }

    #[test]
    fn driving_distance_nulls_are_zero_rows_and_a_negative_limit() {
        assert_eq!(DrivingDistanceAggregate::new().finish().unwrap(), None);
        assert_eq!(driving(&[(10, 1, 2, 1.1, None)], 1, -1.0), None);
    }

    #[test]
    fn driving_distance_endpoints_must_be_constant() {
        let mut acc = DrivingDistanceAggregate::new();
        acc.step(1, 1, 2, 1.0, 1, 5.0, None).unwrap();
        let err = acc.step(2, 2, 3, 1.0, 1, 6.0, None).unwrap_err();
        assert!(err.to_string().contains("must be constant"), "{err}");
    }
}
