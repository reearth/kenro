//! pgRouting-style shortest paths as aggregate functions: the `WHERE`
//! clause plays the role of pgRouting's SQL-string argument, and the
//! accumulator materializes the edge set before searching — exactly what
//! pgRouting itself does with the rows its query returns.
//!
//! `kenro_dijkstra(id, source, target, cost, start_vid, end_vid
//! [, reverse_cost])` deliberately diverges from `pgr_dijkstra`'s column
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

/// Textbook Dijkstra with a predecessor trail. `None` when `start` or `end`
/// does not appear in the edge set, or no path exists. `start == end`
/// returns `None` too, matching `pgr_dijkstra`'s empty set.
fn shortest_path(edges: &[Edge], start: i32, end: i32) -> Option<Vec<PathStep>> {
    if start == end {
        return None;
    }
    // Dense-remap node ids; BTreeMap so the numbering — and therefore the
    // heap tie-break — is deterministic whatever the row order was.
    let mut index: BTreeMap<i32, usize> = BTreeMap::new();
    for e in edges {
        let n = index.len();
        index.entry(e.source).or_insert(n);
        let n = index.len();
        index.entry(e.target).or_insert(n);
    }
    let start_ix = *index.get(&start)?;
    let end_ix = *index.get(&end)?;

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

    let n = index.len();
    let mut dist = vec![f64::INFINITY; n];
    let mut settled = vec![false; n];
    // (previous node, edge id taken, that edge's cost)
    let mut prev: Vec<Option<(usize, i32, f64)>> = vec![None; n];
    let mut heap = BinaryHeap::new();
    dist[start_ix] = 0.0;
    heap.push(Item {
        dist: 0.0,
        node: start_ix,
    });
    while let Some(Item { dist: d, node }) = heap.pop() {
        if settled[node] {
            continue;
        }
        settled[node] = true;
        if node == end_ix {
            break;
        }
        for &(to, cost, id) in &adj[node] {
            let nd = d + cost;
            if nd < dist[to] {
                dist[to] = nd;
                prev[to] = Some((node, id, cost));
                heap.push(Item { dist: nd, node: to });
            }
        }
    }
    if !settled[end_ix] {
        return None;
    }

    // Walk the trail back, then emit rows front-to-back: each row is the
    // node reached and the edge leaving it, the terminal row carries -1.
    let by_index: Vec<i32> = {
        let mut v = vec![0; n];
        for (&node, &ix) in &index {
            v[ix] = node;
        }
        v
    };
    let mut hops = Vec::new(); // (from index, edge id, edge cost)
    let mut at = end_ix;
    while at != start_ix {
        let (from, id, cost) = prev[at].expect("settled node has a trail");
        hops.push((from, id, cost));
        at = from;
    }
    hops.reverse();
    let mut steps = Vec::with_capacity(hops.len() + 1);
    let mut agg = 0.0;
    for (from, id, cost) in hops {
        steps.push(PathStep {
            node: by_index[from],
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
}
