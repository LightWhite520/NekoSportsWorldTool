//! A confirmation binds geometry to the map and the current checkpoint context.
use route_planner::{athletics::TrackCandidate, RoadGraph};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TrackSelection {
    pub candidate: TrackCandidate,
    pub context: String,
    pub confirmed: bool,
    #[serde(default)]
    pub geometry: String,
}

pub fn geometry(candidate: &TrackCandidate) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(candidate).unwrap_or_default())
    )
}

pub fn context(
    net: &RoadGraph,
    school: &str,
    fences: &[Vec<(f64, f64)>],
    points: &[(f64, f64)],
) -> String {
    use route_planner::graph::Coord;
    let c = |c: Coord| format!("{:.9},{:.9}", c.lon, c.lat);
    let mut edges: Vec<_> = net
        .graph
        .edge_indices()
        .map(|e| {
            let (a, b) = net.graph.edge_endpoints(e).unwrap();
            format!(
                "{}>{}:{}:{}",
                c(net.node_coord(a)),
                c(net.node_coord(b)),
                net.graph[e].way_id,
                net.graph[e].oneway
            )
        })
        .collect();
    edges.sort();
    let mut tracks: Vec<_> = net
        .tracks
        .iter()
        .map(|t| format!("{}:{}:{:?}", t.id, t.counterclockwise_allowed, t.points))
        .collect();
    tracks.sort();
    let mut h = Sha256::new();
    h.update(b"track-confirmation-v1");
    h.update(school);
    h.update(format!("{fences:?}{points:?}{edges:?}{tracks:?}"));
    format!("{:x}", h.finalize())
}

impl TrackSelection {
    pub fn resolve(
        &self,
        net: &RoadGraph,
        school: &str,
        fences: &[Vec<(f64, f64)>],
        points: &[(f64, f64)],
    ) -> Result<TrackCandidate, String> {
        if !self.confirmed
            || self.geometry != geometry(&self.candidate)
            || self.context != context(net, school, fences, points)
        {
            return Err("跑道尚未确认或地图/点位已变化，请刷新预览并重新确认".into());
        }
        if self.candidate.source != "manual"
            && !net.tracks.iter().any(|t| {
                t.id == self.candidate.id
                    && t.points == self.candidate.points
                    && t.counterclockwise_allowed == self.candidate.counterclockwise_allowed
            })
        {
            return Err("已确认跑道不在当前地图中".into());
        }
        let mut candidate = self.candidate.clone();
        candidate.points = route_planner::athletics::normalize(&candidate.points)?;
        for c in &mut candidate.points {
            let (a, b) = super::geom::wgs84_to_bd09(c.lat, c.lon);
            c.lat = a;
            c.lon = b;
        }
        Ok(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_expires_with_school_points_map_or_geometry() {
        let xml=br#"<osm><node id="1" lat="30" lon="10"/><node id="2" lat="30" lon="10.001"/><node id="3" lat="30.001" lon="10"/><way id="1"><nd ref="1"/><nd ref="2"/><nd ref="3"/><nd ref="1"/><tag k="leisure" v="track"/></way></osm>"#;
        let mut net = route_planner::load_osm(xml).unwrap();
        let candidate = net.tracks[0].clone();
        let points = [(30., 10.)];
        let mut s = TrackSelection {
            context: context(&net, "school-a", &[], &points),
            confirmed: true,
            geometry: geometry(&candidate),
            candidate,
        };
        assert!(s.resolve(&net, "school-a", &[], &points).is_ok());
        assert!(s.resolve(&net, "school-b", &[], &points).is_err());
        assert!(s.resolve(&net, "school-a", &[], &[(30., 10.0001)]).is_err());
        s.candidate.points[1].lat += 0.00001;
        assert!(s.resolve(&net, "school-a", &[], &points).is_err());
        s.candidate = net.tracks[0].clone();
        net.graph.clear_edges();
        assert!(s.resolve(&net, "school-a", &[], &points).is_err());
        let old: crate::api::model::Config =
            serde_json::from_str(r#"{"route_mode":"road"}"#).unwrap();
        assert!(!old.track_enabled);
        assert!(old.track_selection.is_none());
    }
}
