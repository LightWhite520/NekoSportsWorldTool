//! Road geometry and confirmed athletics circuits, sharing the motion adapter.

#[cfg(test)]
use super::geom::MET_PER_DEG_LNG;
use super::geom::{wgs84_to_bd09, MET_PER_DEG_LAT};
use super::model::Track;
use route_planner::{plan_route_split, Coord, RoadGraph, Route, RouteOptions};

/// 路线算法模式（二选一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RouteMode {
    /// 经典打卡点环（Catmull-Rom）。
    #[default]
    Legacy,
    /// 真实道路拓扑路由。
    Road,
}

impl RouteMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "road" => RouteMode::Road,
            _ => RouteMode::Legacy,
        }
    }
}

/// 从本地文件加载 OSM 路网。
pub fn load_network_path(path: &str) -> Result<RoadGraph, String> {
    if path.trim().is_empty() {
        return Err("未配置 OSM 路网，请先导入路网或主动切换经典模式".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读取 {path} 失败: {e}"))?;
    let xml = std::str::from_utf8(&bytes)
        .map_err(|_| "文件不是 OSM XML；PBF 等二进制格式请先转换为 .osm XML")?;
    if !xml.contains("<osm") || !xml.contains("</osm>") || xml.contains("<remark") {
        return Err("文件不是完整的 OSM XML 或包含地图服务错误；请重新导出 .osm/.xml".into());
    }
    let net = route_planner::load_osm(&bytes)?;
    if net.graph.edge_count() == 0
        || net.graph.node_weights().any(|n| {
            !n.coord.lat.is_finite()
                || !n.coord.lon.is_finite()
                || n.coord.lat.abs() > 90.
                || n.coord.lon.abs() > 180.
        })
    {
        return Err("路网没有可用道路或包含无效坐标，请检查 OSM 文件".into());
    }
    Ok(net)
}

/// 路网预览计划（供地图显示）：对齐后的路网/建筑 + 生成的路线（BD 系）。
pub struct RoadPlan {
    pub prefix_m: f64,
    pub circuit_m: f64,
    pub track_mode: bool,
    pub arcs: Vec<f64>,
    pub laps: Vec<f64>,
    pub route: Vec<(f64, f64)>,
    pub length_m: f64,
    pub edges: Vec<Vec<(f64, f64)>>,
    pub buildings: Vec<Vec<(f64, f64)>>,
    pub checkpoints: Vec<(f64, f64)>,
    pub fences: Vec<Vec<(f64, f64)>>,
}

/// 用围栏（BD 系，与打卡点同系）裁剪路网：仅保留围栏内道路；空路网交由路由器明确报错。
pub fn apply_fences(net: &RoadGraph, fences: &[Vec<(f64, f64)>]) -> RoadGraph {
    if fences.is_empty() {
        return net.clone();
    }
    let mut g = net.clone();
    let polys: Vec<Vec<Coord>> = fences
        .iter()
        .map(|r| r.iter().map(|p| Coord::new(p.1, p.0)).collect())
        .collect();
    g.boundaries = polys.clone();
    g.graph
        .retain_nodes(|graph, n| route_planner::boundary::contains(&polys, graph[n].coord));
    // 进一步裁剪：移除「穿越围栏边界」的边（端点在内但中段跑出围栏，如跨校区道路）。
    g.graph.retain_edges(|graph, e| {
        let (a, b) = graph.edge_endpoints(e).unwrap();
        edge_inside(&polys, graph[a].coord, graph[b].coord)
    });
    g.rebuild_index();
    g
}

fn edge_inside(polys: &[Vec<Coord>], a: Coord, b: Coord) -> bool {
    route_planner::boundary::segment_inside(polys, a, b)
}

/// 生成路网预览（对齐 + 围栏裁剪 + 闭环路由），不产生协议字段。
///
/// 调用方提供合并后的 policy/实时点位，预览与生成使用同一个路由函数。
pub fn plan_road_view(
    net: &RoadGraph,
    points_bd: &[(f64, f64)],
    dist: f64,
    dur: i64,
    seed: u64,
    fences: &[Vec<(f64, f64)>],
) -> Result<RoadPlan, String> {
    if !fences.is_empty() {
        crate::api::school_roads::validate_fences(fences)?;
    }
    let mut g = net.clone();
    align_network(&mut g);
    g = apply_fences(&g, fences);
    let ordered = points_bd.to_vec();
    let route = required_route(&g, &ordered, dist, dur, seed)?;
    route_planner::motion::generate(
        &route_planner::itinerary::Itinerary::looping(route.clone()).expand(dist)?,
        dur,
        seed,
    )?;
    let route_pts: Vec<(f64, f64)> = route.points.iter().map(|p| (p.lat, p.lon)).collect();
    let edges: Vec<Vec<(f64, f64)>> = g
        .edges_deg()
        .iter()
        .map(|e| e.iter().map(|c| (c.lat, c.lon)).collect())
        .collect();
    let buildings: Vec<Vec<(f64, f64)>> = g
        .buildings
        .iter()
        .map(|r| r.iter().map(|c| (c.lat, c.lon)).collect())
        .collect();
    Ok(RoadPlan {
        prefix_m: 0.,
        circuit_m: route.length_m,
        track_mode: false,
        arcs: route.points.iter().map(|p| p.s).collect(),
        laps: super::route_view::laps(&route, dist),
        route: route_pts,
        length_m: route.length_m,
        edges,
        buildings,
        checkpoints: ordered,
        fences: fences.to_vec(),
    })
}

/// 将 WGS84 路网逐节点转换为 BD-09（与打卡点同系）。
///
/// OSM 为 WGS84，打卡点为 BD-09。两者不是常数平移关系（境内 GCJ/BD 偏移随位置
/// 变化），故用标准 WGS84→GCJ-02→BD-09 变换逐节点转换，校园尺度下与真实道路
/// 对齐（优于旧版质心常数平移的米级~数十米残差）。
pub fn align_network(net: &mut RoadGraph) {
    if net.graph.node_count() == 0 {
        return;
    }
    for track in &mut net.tracks {
        for c in &mut track.points {
            let (lat, lon) = wgs84_to_bd09(c.lat, c.lon);
            c.lat = lat;
            c.lon = lon;
        }
    }
    for node in net.graph.node_weights_mut() {
        let (lat, lon) = wgs84_to_bd09(node.coord.lat, node.coord.lon);
        node.coord.lat = lat;
        node.coord.lon = lon;
    }
    for ring in net.buildings.iter_mut() {
        for c in ring.iter_mut() {
            let (lat, lon) = wgs84_to_bd09(c.lat, c.lon);
            c.lat = lat;
            c.lon = lon;
        }
    }
    net.compute_anchor();
    net.rebuild_index();
}

/// 按质心角度排序打卡点，形成自然环序。
pub fn radial_order(points_bd: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let n = points_bd.len();
    if n == 0 {
        return Vec::new();
    }
    let (cx, cy) = (
        points_bd.iter().map(|q| q.0).sum::<f64>() / n as f64,
        points_bd.iter().map(|q| q.1).sum::<f64>() / n as f64,
    );
    let mut ordered = points_bd.to_vec();
    ordered.sort_by(|a, b| {
        (a.0 - cx)
            .atan2(a.1 - cy)
            .partial_cmp(&(b.0 - cx).atan2(b.1 - cy))
            .unwrap()
    });
    ordered
}

fn required_route(
    net: &RoadGraph,
    points: &[(f64, f64)],
    dist: f64,
    dur: i64,
    seed: u64,
) -> Result<Route, String> {
    if !dist.is_finite() || dist <= 0.0 || dur <= 0 {
        return Err("距离和时长必须为正数".into());
    }
    let avg = dist / dur as f64;
    let opts = RouteOptions {
        a_c_max: 2.5,
        min_radius_m: (avg * avg / 2.5).max(6.0),
        repeat_checkpoint_loop: true,
        ..Default::default()
    };
    let waypoints: Vec<Coord> = points.iter().map(|p| Coord::new(p.1, p.0)).collect();
    if !net.boundaries.is_empty()
        && waypoints
            .iter()
            .any(|p| !route_planner::boundary::contains(&net.boundaries, *p))
    {
        return Err("点位在学校围栏外，无法生成校内路线".into());
    }
    let check = route_planner::check::inspect(net, &waypoints, dist);
    if let Some(error) = check.error() {
        return Err(error);
    }
    let route = plan_route_split(net, &waypoints, &waypoints, dist, seed, &opts)?;
    if route.points.len() < 2 || !route.length_m.is_finite() || route.length_m <= 0.0 {
        return Err("路网无法形成有效闭环".into());
    }
    for (i, &(lat, lon)) in points.iter().enumerate() {
        let nearest = route
            .points
            .iter()
            .map(|p| {
                let dy = (p.lat - lat) * MET_PER_DEG_LAT;
                let dx = (p.lon - lon) * 111_320.0 * lat.to_radians().cos();
                dx.hypot(dy)
            })
            .fold(f64::INFINITY, f64::min);
        if nearest > 40.0 {
            return Err(format!(
                "路网无法到达第 {} 个点位（最近 {:.1} 米，超过 40 米）",
                i + 1,
                nearest
            ));
        }
    }
    Ok(route)
}

/// 模式 B：真实道路拓扑路由轨迹生成。
///
/// `route_bd[0]` 为起点；`route_bd` 和 `must_bd` 合并为强制必经点。
/// 不使用离路吸附；平滑路线和最终采样均须经过全部点位的 40 米范围。
pub fn build_road(
    dist: f64,
    dur: i64,
    seed: u64,
    start_ms: i64,
    route_bd: &[(f64, f64)],
    must_bd: &[(f64, f64)],
    net: &RoadGraph,
) -> Result<Track, String> {
    let mut required = route_bd.to_vec();
    for p in must_bd {
        if !required.contains(p) {
            required.push(*p);
        }
    }
    let route = required_route(net, &required, dist, dur, seed)?;
    let expanded = route_planner::itinerary::Itinerary::looping(route).expand(dist)?;
    let track = super::motion::build(&expanded, dur, seed, start_ms, &net.boundaries)?;
    for &point in &required {
        if !super::wire::passes_point(&track, point) {
            return Err("最终轨迹未覆盖全部必经点".into());
        }
    }
    Ok(track)
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    #[test]
    fn preview_and_generation_share_required_points_options_and_seed() {
        let mut net = RoadGraph::new();
        let wgs = [
            (39.9, 116.4),
            (39.9, 116.402),
            (39.902, 116.402),
            (39.902, 116.4),
        ];
        let nodes: Vec<_> = wgs
            .iter()
            .map(|&(lat, lon)| net.add_node(Coord::new(lon, lat)))
            .collect();
        for i in 0..4 {
            let edge = route_planner::graph::EdgeData {
                way_id: i as u64,
                oneway: false,
                highway: "footway".into(),
                maxspeed_kmh: None,
                length_m: 200.0,
            };
            net.add_edge(nodes[i], nodes[(i + 1) % 4], edge.clone());
            net.add_edge(nodes[(i + 1) % 4], nodes[i], edge);
        }
        net.compute_anchor();
        net.rebuild_index();
        let points: Vec<_> = wgs.iter().map(|&(a, b)| wgs84_to_bd09(a, b)).collect();
        let fences = vec![vec![
            (39.89, 116.39),
            (39.89, 116.42),
            (39.92, 116.42),
            (39.92, 116.39),
        ]
        .into_iter()
        .map(|(a, b)| wgs84_to_bd09(a, b))
        .collect()];
        let preview = plan_road_view(&net, &points, 2000.0, 800, 7, &fences).unwrap();
        let mut aligned = net.clone();
        align_network(&mut aligned);
        let aligned = apply_fences(&aligned, &fences);
        let route = required_route(&aligned, &points, 2000.0, 800, 7).unwrap();
        assert_eq!(preview.checkpoints, points);
        assert_eq!(
            preview.route,
            route
                .points
                .iter()
                .map(|p| (p.lat, p.lon))
                .collect::<Vec<_>>()
        );
        let track = build_road(2000.0, 800, 7, 0, &points, &points, &aligned).unwrap();
        assert!(points
            .iter()
            .all(|p| super::super::wire::passes_point(&track, *p)));
        for p in &track.locations {
            let nearest = preview
                .route
                .windows(2)
                .map(|w| {
                    let xy = |(lat, lon): (f64, f64)| {
                        [
                            (lon - p.gLng) * MET_PER_DEG_LNG,
                            (lat - p.gLat) * MET_PER_DEG_LAT,
                        ]
                    };
                    let (q, _) =
                        route_planner::graph::project_to_segment([0.0, 0.0], xy(w[0]), xy(w[1]));
                    q[0].hypot(q[1])
                })
                .fold(f64::INFINITY, f64::min);
            assert!(nearest < 40.0, "preview/generation mismatch: {nearest}");
        }
    }

    // A concave campus with a road at its re-entrant corner: connecting timed
    // observations without preserving this vertex would leave campus.
    fn concave_campus() -> (RoadGraph, Vec<Vec<(f64, f64)>>, Vec<(f64, f64)>) {
        let coord = |(x, y): (f64, f64)| (39.9 + y / 111132.0, 116.4 + x / 85400.0);
        let fence = vec![
            (0., 0.),
            (240., 0.),
            (240., 100.),
            (100., 100.),
            (100., 240.),
            (0., 240.),
        ]
        .into_iter()
        .map(coord)
        .collect();
        let path: Vec<_> = vec![
            (10., 10.),
            (230., 10.),
            (230., 90.),
            (90., 90.),
            (90., 230.),
            (10., 230.),
        ]
        .into_iter()
        .map(coord)
        .collect();
        let mut net = RoadGraph::new();
        let nodes: Vec<_> = path
            .iter()
            .map(|&(lat, lon)| net.add_node(Coord::new(lon, lat)))
            .collect();
        net.compute_anchor();
        for i in 0..nodes.len() {
            let j = (i + 1) % nodes.len();
            let a = net.to_m(net.node_coord(nodes[i]));
            let b = net.to_m(net.node_coord(nodes[j]));
            let edge = route_planner::graph::EdgeData {
                way_id: i as u64,
                oneway: false,
                highway: "footway".into(),
                maxspeed_kmh: None,
                length_m: (a[0] - b[0]).hypot(a[1] - b[1]),
            };
            net.add_edge(nodes[i], nodes[j], edge.clone());
            net.add_edge(nodes[j], nodes[i], edge);
        }
        net.rebuild_index();
        (net, vec![fence], path)
    }

    #[test]
    fn long_distance_repeats_inside_concave_campus_and_keeps_statistics() {
        let (net, fences, points) = concave_campus();
        let net = apply_fences(&net, &fences);
        for seed in 0..16 {
            // Campus perimeter is 960 m; 5 km must reuse school roads.
            let track = build_road(5000.0, 2000, seed, 123000, &points, &points, &net).unwrap();
            assert!((track.totalDistance - 5000.0).abs() < 0.001);
            assert_eq!(track.totalTime, 2000);
            track.validate_consistency().unwrap();
            assert!(track
                .locations
                .windows(2)
                .all(|w| route_planner::boundary::segment_inside(
                    &net.boundaries,
                    Coord::new(w[0].gLng, w[0].gLat),
                    Coord::new(w[1].gLng, w[1].gLat)
                )));
            assert!(points
                .iter()
                .all(|p| super::super::wire::passes_point(&track, *p)));
            assert_eq!(track.locations.last().unwrap().steps, track.totalSteps);
        }
    }

    #[test]
    fn no_boundary_extra_mileage_reuses_the_same_checkpoint_circuit() {
        let (mut net, _, points) = concave_campus();
        // A long, returnable side road must not be explored to fill the mileage.
        let start = net.graph.node_indices().next().unwrap();
        let far = net.add_node(Coord::new(116.42, 39.9));
        let edge = route_planner::graph::EdgeData {
            way_id: 99,
            oneway: false,
            highway: "footway".into(),
            maxspeed_kmh: None,
            length_m: 1700.,
        };
        net.add_edge(start, far, edge.clone());
        net.add_edge(far, start, edge);
        net.rebuild_index();
        assert!(net.boundaries.is_empty());
        let short = required_route(&net, &points, 2000., 800, 7).unwrap();
        let long = required_route(&net, &points, 5000., 2000, 19).unwrap();
        assert_eq!(short.length_m, long.length_m);
        assert!(long.length_m < 1100.);
        assert_eq!(
            short
                .points
                .iter()
                .map(|p| (p.lat, p.lon, p.s))
                .collect::<Vec<_>>(),
            long.points
                .iter()
                .map(|p| (p.lat, p.lon, p.s))
                .collect::<Vec<_>>()
        );
        let laps = super::super::route_view::laps(&long, 5000.);
        assert!(laps.len() >= 5);
        assert!((laps.iter().sum::<f64>() - 5000.).abs() < 0.001);
        for seed in 0..4 {
            let track = build_road(5000., 2000, seed, 123000, &points, &points, &net).unwrap();
            track.validate_consistency().unwrap();
            assert!((track.totalDistance - 5000.).abs() < 0.001);
            assert_eq!(track.totalTime, 2000);
            assert!(track.locations.iter().all(|p| p.gLng < 116.405));
            assert!(points
                .iter()
                .all(|p| super::super::wire::passes_point(&track, *p)));
        }
    }

    #[test]
    fn supplied_boundary_still_rejects_outside_points() {
        let (net, fences, mut points) = concave_campus();
        let clipped = apply_fences(&net, &fences);
        points.push((40.0, 117.0));
        assert!(build_road(5000., 2000, 1, 0, &points, &points, &clipped).is_err());
    }

    #[test]
    fn preflight_reports_short_distance_and_marks_offroad_point() {
        let (net, fences, points) = concave_campus();
        let net = apply_fences(&net, &fences);
        let cp: Vec<_> = points.iter().map(|&(a, b)| Coord::new(b, a)).collect();
        let check = route_planner::check::inspect(&net, &cp, 100.);
        assert_eq!(check.components, 1);
        assert!(check.minimum_m.unwrap() > 800.);
        assert!(check.error().unwrap().contains("不足"));
        let bad = route_planner::check::inspect(&net, &[Coord::new(117., 40.)], 5000.);
        assert_eq!(bad.issues[0].point, Some(0));
        assert!(bad.issues[0].position.is_some());
    }

    #[test]
    fn required_points_in_disconnected_school_roads_fail() {
        let (mut net, fences, points) = concave_campus();
        net.graph.clear_edges();
        let nodes: Vec<_> = net.graph.node_indices().collect();
        for (a, b) in [(0, 1), (3, 4)] {
            let edge = route_planner::graph::EdgeData {
                way_id: a as u64,
                oneway: false,
                highway: "footway".into(),
                maxspeed_kmh: None,
                length_m: 200.0,
            };
            net.add_edge(nodes[a], nodes[b], edge.clone());
            net.add_edge(nodes[b], nodes[a], edge);
        }
        net.rebuild_index();
        let net = apply_fences(&net, &fences);
        assert!(build_road(5000., 2000, 1, 0, &[points[0], points[3]], &[], &net).is_err());
    }
}

/// Both preview and generation consume this same finite journey.
pub fn track_itinerary(
    net: &RoadGraph,
    points: &[(f64, f64)],
    dist: f64,
    candidate: &route_planner::athletics::TrackCandidate,
) -> Result<route_planner::itinerary::Itinerary, String> {
    route_planner::athletics::plan(
        net,
        &points
            .iter()
            .map(|&(a, b)| Coord::new(b, a))
            .collect::<Vec<_>>(),
        candidate,
        dist,
    )
}
pub fn plan_track_view(
    net: &RoadGraph,
    points: &[(f64, f64)],
    dist: f64,
    dur: i64,
    seed: u64,
    fences: &[Vec<(f64, f64)>],
    candidate: &route_planner::athletics::TrackCandidate,
) -> Result<RoadPlan, String> {
    if !fences.is_empty() {
        crate::api::school_roads::validate_fences(fences)?;
    }
    let mut aligned = net.clone();
    align_network(&mut aligned);
    let g = apply_fences(&aligned, fences);
    let itinerary = track_itinerary(&g, points, dist, candidate)?;
    let route = itinerary.expand(dist)?;
    route_planner::motion::generate(&route, dur, seed)?;
    let mut laps = Vec::new();
    let mut remaining = dist - itinerary.prefix.length_m;
    while remaining > 1e-6 {
        let d = remaining.min(itinerary.circuit.length_m);
        laps.push(d);
        remaining -= d;
    }
    Ok(RoadPlan {
        prefix_m: itinerary.prefix.length_m,
        circuit_m: itinerary.circuit.length_m,
        track_mode: true,
        arcs: route.points.iter().map(|p| p.s).collect(),
        route: route.points.iter().map(|p| (p.lat, p.lon)).collect(),
        length_m: dist,
        laps,
        edges: g
            .edges_deg()
            .iter()
            .map(|e| e.iter().map(|p| (p.lat, p.lon)).collect())
            .collect(),
        buildings: g
            .buildings
            .iter()
            .map(|e| e.iter().map(|p| (p.lat, p.lon)).collect())
            .collect(),
        checkpoints: points.to_vec(),
        fences: fences.to_vec(),
    })
}
pub fn build_track(
    net: &RoadGraph,
    points: &[(f64, f64)],
    dist: f64,
    dur: i64,
    seed: u64,
    start_ms: i64,
    candidate: &route_planner::athletics::TrackCandidate,
) -> Result<Track, String> {
    let route = track_itinerary(net, points, dist, candidate)?.expand(dist)?;
    let track = super::motion::build(&route, dur, seed, start_ms, &net.boundaries)?;
    if points
        .iter()
        .any(|&p| !super::wire::passes_point(&track, p))
    {
        return Err("最终轨迹未经过全部必经点".into());
    }
    Ok(track)
}

#[cfg(test)]
mod athletics_integration_tests {
    use super::*;
    #[test]
    fn confirmed_wgs_map_preview_and_bd_generation_agree() {
        let xml=br#"<osm><node id="1" lat="30" lon="10"/><node id="2" lat="30" lon="10.001"/><node id="3" lat="30.001" lon="10.001"/><node id="4" lat="30.001" lon="10"/><node id="5" lat="30" lon="9.999"/><way id="1"><nd ref="1"/><nd ref="2"/><nd ref="3"/><nd ref="4"/><nd ref="1"/><tag k="leisure" v="track"/></way><way id="2"><nd ref="5"/><nd ref="1"/><tag k="highway" v="footway"/></way></osm>"#;
        let net = route_planner::load_osm(xml).unwrap();
        let points = vec![wgs84_to_bd09(30., 9.999), wgs84_to_bd09(30., 10.)];
        let candidate = net.tracks[0].clone();
        let selection = crate::track::selection::TrackSelection {
            context: crate::track::selection::context(&net, "fixture", &[], &points),
            geometry: crate::track::selection::geometry(&candidate),
            candidate,
            confirmed: true,
        };
        let candidate = selection.resolve(&net, "fixture", &[], &points).unwrap();
        let preview = plan_track_view(&net, &points, 2000., 800, 7, &[], &candidate).unwrap();
        assert!(preview.prefix_m > 90.);
        assert!((preview.prefix_m + preview.laps.iter().sum::<f64>() - 2000.).abs() < 1e-6);
        let mut aligned = net.clone();
        align_network(&mut aligned);
        let track = build_track(&aligned, &points, 2000., 800, 7, 0, &candidate).unwrap();
        track.validate_consistency().unwrap();
        for &cp in &points {
            assert!(crate::track::wire::passes_point(&track, cp));
        }
        let end = track.locations.last().unwrap();
        assert!(
            crate::track::analysis::distance((end.gLat, end.gLng), *preview.route.last().unwrap())
                < 3.
        );
        assert!(plan_track_view(&net, &points, 2000., 10, 7, &[], &candidate).is_err());
    }
}
