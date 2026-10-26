//! Explicit, confirmed running circuits. No proximity edges are invented.
use crate::{
    graph::{dist, Coord, EdgeData, RoadGraph},
    itinerary::{polyline, Itinerary},
    route::shortest_path,
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TrackCandidate {
    pub id: String,
    pub source: String,
    pub points: Vec<Coord>,
    pub counterclockwise_allowed: bool,
}

pub fn normalize(points: &[Coord]) -> Result<Vec<Coord>, String> {
    let mut p = points.to_vec();
    if p.first() != p.last() {
        if let Some(&first) = p.first() {
            p.push(first);
        }
    }
    if p.len() < 4 || p.len() > 4096 {
        return Err("跑道需要 3 至 4095 个有效顶点".into());
    }
    polyline(&p)?;
    let origin = p[0];
    let xy = |c: Coord| {
        [
            (c.lon - origin.lon) * 111320. * origin.lat.to_radians().cos(),
            (c.lat - origin.lat) * 111132.,
        ]
    };
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    for i in 0..p.len() - 1 {
        if dist(xy(p[i]), xy(p[i + 1])) < 0.05 {
            return Err("跑道含重复或过近节点".into());
        }
        for j in i + 2..p.len() - 1 {
            if i == 0 && j == p.len() - 2 {
                continue;
            }
            let (a, b, c, d) = (xy(p[i]), xy(p[i + 1]), xy(p[j]), xy(p[j + 1]));
            let overlap = a[0].min(b[0]) <= c[0].max(d[0])
                && c[0].min(d[0]) <= a[0].max(b[0])
                && a[1].min(b[1]) <= c[1].max(d[1])
                && c[1].min(d[1]) <= a[1].max(b[1]);
            if overlap
                && cross(a, b, c) * cross(a, b, d) <= 0.
                && cross(c, d, a) * cross(c, d, b) <= 0.
            {
                return Err("跑道轮廓自交或重叠".into());
            }
        }
    }
    let area: f64 = p
        .windows(2)
        .map(|w| {
            let a = xy(w[0]);
            let b = xy(w[1]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    if area.abs() < 2. {
        return Err("跑道面积退化".into());
    }
    if area < 0. {
        p.reverse();
    }
    Ok(p)
}

pub fn plan(
    net: &RoadGraph,
    checkpoints: &[Coord],
    selected: &TrackCandidate,
    distance: f64,
) -> Result<Itinerary, String> {
    if checkpoints.is_empty()
        || checkpoints.iter().any(|c| {
            !c.lon.is_finite() || !c.lat.is_finite() || c.lat.abs() > 85. || c.lon.abs() > 180.
        })
        || !distance.is_finite()
        || distance <= 0.
        || distance > 100_000.
    {
        return Err("起点、必经点或目标距离无效".into());
    }
    if !selected.counterclockwise_allowed {
        return Err("跑道通行方向不允许逆时针".into());
    }
    let ring = normalize(&selected.points)?;
    if !net.boundaries.is_empty()
        && (!ring
            .windows(2)
            .all(|w| crate::boundary::segment_inside(&net.boundaries, w[0], w[1]))
            || checkpoints
                .iter()
                .any(|&c| !crate::boundary::contains(&net.boundaries, c)))
    {
        return Err("跑道或点位越出学校围栏".into());
    }
    let mut g = net.clone();
    // Manual geometry is a declared track, not a connector. Only exact shared
    // coordinates join existing nodes; otherwise the caller must repair the map.
    if selected.source == "manual" {
        let mut nodes = Vec::new();
        for &c in ring.iter().take(ring.len() - 1) {
            let n = g
                .graph
                .node_indices()
                .find(|&n| dist(g.to_m(g.node_coord(n)), g.to_m(c)) < 0.05);
            nodes.push(n.unwrap_or_else(|| g.add_node(c)));
        }
        for i in 0..nodes.len() {
            let (a, b) = (nodes[i], nodes[(i + 1) % nodes.len()]);
            if g.graph.find_edge(a, b).is_none()
                && g.graph
                    .find_edge(b, a)
                    .map(|e| g.graph[e].oneway)
                    .unwrap_or(false)
            {
                return Err("手动跑道与地图的单行方向冲突".into());
            }
            let length_m = dist(g.to_m(g.node_coord(a)), g.to_m(g.node_coord(b)));
            g.add_edge(
                a,
                b,
                EdgeData {
                    way_id: u64::MAX,
                    oneway: true,
                    highway: "running_track".into(),
                    maxspeed_kmh: None,
                    length_m,
                },
            );
        }
        g.rebuild_index();
    }
    // Direct every selected circuit edge CCW before planning the prefix too.
    // This also detects stale/missing edges after boundary clipping.
    let mut ring_nodes = Vec::new();
    for &c in &ring {
        let n = g
            .graph
            .node_indices()
            .find(|&n| dist(g.to_m(g.node_coord(n)), g.to_m(c)) < 0.05)
            .ok_or("跑道节点缺失，请刷新地图")?;
        ring_nodes.push(n);
    }
    for w in ring_nodes.windows(2) {
        if g.graph.find_edge(w[0], w[1]).is_none() {
            return Err("跑道缺少逆时针可通行路段".into());
        }
    }
    g.graph.retain_edges(|graph, e| {
        let (a, b) = graph.edge_endpoints(e).unwrap();
        !ring_nodes.windows(2).any(|w| w[1] == a && w[0] == b)
    });
    g.rebuild_index();
    let cp = crate::project::add_virtual_nodes(&mut g, checkpoints)?;
    let mut path = vec![cp[0]];
    for pair in cp.windows(2) {
        let seg = shortest_path(&g, pair[0], pair[1]).ok_or("必经点之间道路不连通")?;
        path.extend(seg.into_iter().skip(1));
    }
    let from = *path.last().unwrap();
    let mut best: Option<(f64, usize, Vec<_>)> = None;
    for (i, &c) in ring.iter().take(ring.len() - 1).enumerate() {
        let Some(n) = g
            .graph
            .node_indices()
            .find(|&n| dist(g.to_m(g.node_coord(n)), g.to_m(c)) < 0.05)
        else {
            continue;
        };
        if let Some(seg) = shortest_path(&g, from, n) {
            let length: f64 = seg
                .windows(2)
                .map(|w| dist(g.to_m(g.node_coord(w[0])), g.to_m(g.node_coord(w[1]))))
                .sum();
            if best.as_ref().map(|b| length < b.0).unwrap_or(true) {
                best = Some((length, i, seg));
            }
        }
    }
    let (_, entry, seg) = best.ok_or("道路无法连接到已确认跑道；请补充真实入口道路")?;
    path.extend(seg.into_iter().skip(1));
    let prefix_coords: Vec<_> = path.into_iter().map(|n| g.node_coord(n)).collect();
    let mut circuit = ring[..ring.len() - 1].to_vec();
    circuit.rotate_left(entry);
    circuit.push(circuit[0]);
    let circuit = polyline(&circuit)?;
    let mut result = Itinerary::looping(circuit);
    if prefix_coords.len() > 1 {
        result.prefix = polyline(&prefix_coords)?;
    }
    for &c in checkpoints {
        let vertices = result
            .prefix
            .points
            .iter()
            .chain(result.circuit.points.iter());
        if vertices
            .map(|p| dist(g.to_m(c), g.to_m(Coord::new(p.lon, p.lat))))
            .fold(f64::INFINITY, f64::min)
            > 40.
        {
            return Err("道路不能到达全部必经点的 40 米范围".into());
        }
    }
    result.expand(distance)?;
    Ok(result)
}
