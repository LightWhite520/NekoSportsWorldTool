//! Classic geometry with the shared, checked motion model.
use super::{
    geom::{make_point_ring, to_bd},
    model::Track,
};
use route_planner::{
    itinerary::{polyline, Itinerary},
    Coord,
};
pub fn build(
    dist: f64,
    dur: i64,
    seed: u64,
    _center: (f64, f64),
    start_ms: i64,
    points_bd: &[(f64, f64)],
) -> Result<Track, String> {
    if points_bd.len() < 2
        || points_bd
            .iter()
            .any(|&(a, b)| !a.is_finite() || !b.is_finite() || a.abs() > 85. || b.abs() > 180.)
    {
        return Err("经典路线至少需要两个有效点位".into());
    }
    let (dense, _, center) = make_point_ring(points_bd);
    let mut coords: Vec<_> = dense
        .iter()
        .map(|&(x, y)| {
            let (a, b) = to_bd(x, y, center.0, center.1);
            Coord::new(b, a)
        })
        .collect();
    coords.push(coords[0]);
    let route = Itinerary::looping(polyline(&coords)?).expand(dist)?;
    super::motion::build(&route, dur, seed, start_ms, &[])
}
