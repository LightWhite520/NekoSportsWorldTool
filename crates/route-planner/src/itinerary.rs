//! Geometry consumed by motion: a prefix traversed once, followed by a loop.
use crate::{
    graph::{dist, Coord, RoadGraph},
    Route, RoutePoint,
};

#[derive(Clone, Debug)]
pub struct Itinerary {
    pub prefix: Route,
    pub circuit: Route,
}

pub fn polyline(coords: &[Coord]) -> Result<Route, String> {
    if coords.len() < 2
        || coords.iter().any(|c| {
            !c.lat.is_finite() || !c.lon.is_finite() || c.lat.abs() > 85. || c.lon.abs() > 180.
        })
    {
        return Err("路线坐标无效或不足两个点".into());
    }
    let mut g = RoadGraph::new();
    for &c in coords {
        g.add_node(c);
    }
    g.compute_anchor();
    let mut points: Vec<RoutePoint> = Vec::new();
    let mut s = 0.;
    for &c in coords {
        if let Some(p) = points.last() {
            let d = dist(g.to_m(Coord::new(p.lon, p.lat)), g.to_m(c));
            if d < 1e-6 {
                continue;
            }
            s += d;
        }
        points.push(RoutePoint {
            lon: c.lon,
            lat: c.lat,
            s,
            curvature: 0.,
        });
    }
    if s < 1e-3 {
        return Err("路线长度为零".into());
    }
    // Discrete curvature without moving a corner outside a boundary.
    let closed = coords.first() == coords.last();
    let n = points.len();
    for i in 0..n {
        let neighbors = if i > 0 && i + 1 < n {
            Some((i - 1, i + 1))
        } else if closed && n > 3 {
            Some((n - 2, 1))
        } else {
            None
        };
        if let Some((a, b)) = neighbors {
            let xy = |j: usize| g.to_m(Coord::new(points[j].lon, points[j].lat));
            let (a, p, b) = (xy(a), xy(i), xy(b));
            let u = [p[0] - a[0], p[1] - a[1]];
            let v = [b[0] - p[0], b[1] - p[1]];
            let angle = (u[0] * v[1] - u[1] * v[0])
                .atan2(u[0] * v[0] + u[1] * v[1])
                .abs();
            points[i].curvature = angle / ((dist(a, p) + dist(p, b)) * 0.5).max(1.);
        }
    }
    Ok(Route {
        points,
        length_m: s,
    })
}

pub fn at(route: &Route, s: f64) -> Coord {
    let s = s.clamp(0., route.length_m);
    let i = route
        .points
        .partition_point(|p| p.s < s)
        .clamp(1, route.points.len() - 1);
    let (a, b) = (&route.points[i - 1], &route.points[i]);
    let f = ((s - a.s) / (b.s - a.s)).clamp(0., 1.);
    Coord::new(a.lon + (b.lon - a.lon) * f, a.lat + (b.lat - a.lat) * f)
}

impl Itinerary {
    pub fn looping(circuit: Route) -> Self {
        Self {
            prefix: Route {
                points: vec![],
                length_m: 0.,
            },
            circuit,
        }
    }
    pub fn at(&self, s: f64) -> Coord {
        if s < self.prefix.length_m {
            at(&self.prefix, s)
        } else {
            at(
                &self.circuit,
                (s - self.prefix.length_m).rem_euclid(self.circuit.length_m),
            )
        }
    }
    /// Materialize only the requested journey, keeping all corners and the final partial lap.
    pub fn expand(&self, distance: f64) -> Result<Route, String> {
        if !distance.is_finite()
            || distance <= self.prefix.length_m
            || distance > 100_000.
            || self.circuit.length_m < 1.
        {
            return Err("目标距离不足以完成必经点和跑道连接，或距离超出范围".into());
        }
        let mut points = self.prefix.points.clone();
        let mut offset = self.prefix.length_m;
        while offset < distance {
            for p in &self.circuit.points {
                let s = offset + p.s;
                if s >= distance {
                    break;
                }
                if points.last().map(|q| q.s >= s).unwrap_or(false) {
                    continue;
                }
                let mut q = *p;
                q.s = s;
                points.push(q);
            }
            offset += self.circuit.length_m;
            if points.len() > 500_000 {
                return Err("路线采样规模过大".into());
            }
        }
        let end = self.at(distance);
        points.push(RoutePoint {
            lon: end.lon,
            lat: end.lat,
            s: distance,
            curvature: 0.,
        });
        // Include the connection-to-circuit turn and each seam in the speed envelope.
        let geometry = polyline(
            &points
                .iter()
                .map(|p| Coord::new(p.lon, p.lat))
                .collect::<Vec<_>>(),
        )?;
        if geometry.points.len() == points.len() {
            for (p, g) in points.iter_mut().zip(geometry.points) {
                p.curvature = g.curvature;
            }
        }
        Ok(Route {
            points,
            length_m: distance,
        })
    }
}
