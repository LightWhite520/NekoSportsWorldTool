use route_planner::{graph::EdgeData, Coord, RoadGraph};

/// Synthetic stadium, unrelated to any real school or user location.
pub fn stadium() -> (RoadGraph, Vec<(f64, f64)>) {
    let mut net = RoadGraph::new();
    let mut outline = Vec::new();
    for (cy, first) in [(40.0, 0.0), (-40.0, std::f64::consts::PI)] {
        for i in 0..=24 {
            let angle = first + std::f64::consts::PI * i as f64 / 24.0;
            let x = 36.0 * angle.cos();
            let y = cy + 36.0 * angle.sin();
            outline.push(Coord::new(
                110.0 + x / (111320.0 * 30f64.to_radians().cos()),
                30.0 + y / 111132.0,
            ));
        }
    }
    let nodes: Vec<_> = outline.iter().map(|&c| net.add_node(c)).collect();
    net.compute_anchor();
    for i in 0..nodes.len() {
        let next = (i + 1) % nodes.len();
        let a = net.to_m(outline[i]);
        let b = net.to_m(outline[next]);
        let edge = EdgeData {
            way_id: 1,
            oneway: false,
            highway: "footway".into(),
            maxspeed_kmh: None,
            length_m: (b[0] - a[0]).hypot(b[1] - a[1]),
        };
        net.add_edge(nodes[i], nodes[next], edge.clone());
        net.add_edge(nodes[next], nodes[i], edge);
    }
    net.rebuild_index();
    let points = [0, 12, 25, 37].map(|i| (outline[i].lat, outline[i].lon));
    (net, points.to_vec())
}
