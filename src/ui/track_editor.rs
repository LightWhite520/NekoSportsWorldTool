use super::{map::MapState, run::PreviewInputs, App};
use crate::track::selection::{context, TrackSelection};
use egui::{Color32, Sense, Vec2};
use route_planner::{
    athletics::{normalize, TrackCandidate},
    Coord, RoadGraph,
};

#[cfg(test)]
#[path = "track_editor_tests.rs"]
mod tests;

#[derive(Default)]
pub struct Editor {
    map: MapState,
    fitted: String,
    context_input: String,
    context_key: String,
    editing: bool,
    vertex: Option<usize>,
    undo: Vec<Vec<Coord>>,
    status: String,
}

impl App {
    pub(crate) fn draw_track_editor(
        &mut self,
        ui: &mut egui::Ui,
        net: &RoadGraph,
        inputs: &PreviewInputs,
    ) {
        let mut changed = ui
            .checkbox(
                &mut self.config.track_enabled,
                "跑道模式：必经点后逆时针绕圈",
            )
            .changed();
        if !self.config.track_enabled {
            if changed {
                let _ = crate::api::model::save_config(&self.config);
            }
            return;
        }
        let input_key = format!(
            "{}:{}:{:p}:{:?}:{:?}:{}",
            self.osm_page.request_id,
            inputs.loaded_ms,
            net,
            inputs.fences,
            inputs.points,
            inputs.unid
        );
        if self.run_page.track_editor.context_input != input_key {
            self.run_page.track_editor.context_key =
                context(net, &inputs.unid, &inputs.fences, &inputs.points);
            self.run_page.track_editor.context_input = input_key;
        }
        let key = self.run_page.track_editor.context_key.clone();
        if let Some(s) = &mut self.config.track_selection {
            if s.context != key && s.confirmed {
                s.confirmed = false;
                self.run_page.track_editor.status = "地图或点位已变化，请重新核对跑道".into();
                changed = true;
            }
        }
        ui.label("选择并核对跑道；无合适候选可手动标注。连接段需要地图中的真实道路。");
        let current = self
            .config
            .track_selection
            .as_ref()
            .map(|s| s.candidate.id.clone())
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("track_candidate")
            .selected_text(if current.is_empty() {
                "选择跑道"
            } else {
                &current
            })
            .width(ui.available_width().min(300.))
            .show_ui(ui, |ui| {
                for c in &net.tracks {
                    let source = if c.source == "tagged" {
                        "地图标签"
                    } else {
                        "几何候选，需核对"
                    };
                    if ui
                        .selectable_label(c.id == current, format!("{} · {}", c.id, source))
                        .clicked()
                    {
                        self.config.track_selection = Some(TrackSelection {
                            candidate: c.clone(),
                            context: key.clone(),
                            confirmed: false,
                            geometry: String::new(),
                        });
                        self.run_page.track_editor.editing = false;
                        self.run_page.track_editor.fitted.clear();
                        changed = true;
                    }
                }
            });
        if net.tracks.is_empty() {
            ui.label("当前地图没有可靠闭环；可刷新地图或手动标注。");
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("新建手动跑道").clicked() {
                self.config.track_selection = Some(TrackSelection {
                    candidate: TrackCandidate {
                        id: "manual".into(),
                        source: "manual".into(),
                        points: vec![],
                        counterclockwise_allowed: true,
                    },
                    context: key.clone(),
                    confirmed: false,
                    geometry: String::new(),
                });
                self.run_page.track_editor.editing = true;
                self.run_page.track_editor.vertex = None;
                self.run_page.track_editor.undo.clear();
                changed = true;
            }
            if ui.button("编辑轮廓").clicked() {
                if let Some(s) = &mut self.config.track_selection {
                    if s.candidate.source != "manual" {
                        self.run_page.track_editor.status =
                            "地图跑道保留其原始通行方向；如需修订请新建手动跑道".into();
                    } else {
                        s.confirmed = false;
                        self.run_page.track_editor.editing = true;
                        if s.candidate.points.len() > 1
                            && s.candidate.points.first() == s.candidate.points.last()
                        {
                            s.candidate.points.pop();
                        }
                        changed = true;
                    }
                }
            }
            if ui.button("适配跑道视野").clicked() {
                self.run_page.track_editor.fitted.clear();
            }
        });
        let editor = &mut self.run_page.track_editor;
        let Some(selection) = &mut self.config.track_selection else {
            if changed {
                let _ = crate::api::model::save_config(&self.config);
            }
            return;
        };
        if editor.editing {
            ui.label(
                "点击空白处添加节点；点击节点选中后再点地图移动。靠近道路节点时自动对齐该节点。",
            );
            ui.horizontal_wrapped(|ui| {
                if ui.button("取消节点选择").clicked() {
                    editor.vertex = None;
                }
                if ui.button("删除所选节点").clicked() {
                    if let Some(i) = editor.vertex.take() {
                        if i < selection.candidate.points.len() {
                            editor.undo.push(selection.candidate.points.clone());
                            selection.candidate.points.remove(i);
                            changed = true;
                        }
                    }
                }
                if ui.button("撤销标注").clicked() {
                    if let Some(p) = editor.undo.pop() {
                        selection.candidate.points = p;
                        editor.vertex = None;
                        changed = true;
                    }
                }
                if ui.button("闭合轮廓").clicked() {
                    match normalize(&selection.candidate.points) {
                        Ok(p) => {
                            selection.candidate.points = p;
                            editor.editing = false;
                            editor.vertex = None;
                            editor.status.clear();
                            changed = true;
                        }
                        Err(e) => editor.status = e,
                    }
                }
            });
        }
        let (response, painter) = ui.allocate_painter(
            Vec2::new(ui.available_width(), 240.),
            Sense::click_and_drag(),
        );
        let rect = response.rect;
        if editor.fitted != key {
            let mut bounds: Vec<_> = selection
                .candidate
                .points
                .iter()
                .map(|c| (c.lat, c.lon))
                .collect();
            if bounds.len() < 2 {
                bounds = net
                    .graph
                    .node_weights()
                    .map(|n| (n.coord.lat, n.coord.lon))
                    .collect();
            }
            editor.map.fit(rect, &bounds);
            editor.fitted = key.clone();
        }
        painter.rect_filled(rect, 0., Color32::from_rgb(247, 250, 248));
        editor.map.draw_network(&painter, rect, net);
        let pts: Vec<_> = selection
            .candidate
            .points
            .iter()
            .map(|c| (c.lat, c.lon))
            .collect();
        editor
            .map
            .draw_polyline(&painter, rect, &pts, Color32::from_rgb(20, 150, 110), 2.5);
        for (i, p) in selection.candidate.points.iter().enumerate() {
            editor.map.draw_point(
                &painter,
                rect,
                p.lat,
                p.lon,
                if editor.vertex == Some(i) {
                    Color32::RED
                } else {
                    Color32::DARK_GREEN
                },
                4.,
            );
        }
        if !editor.editing {
            for w in pts.windows(2).step_by((pts.len() / 8).max(1)) {
                let a = editor.map.to_screen(rect, w[0].0, w[0].1);
                let b = editor.map.to_screen(rect, w[1].0, w[1].1);
                let d = b - a;
                if d.length() > 1. {
                    painter.arrow(
                        a + d * 0.4,
                        d.normalized() * 12.,
                        egui::Stroke::new(2_f32, Color32::DARK_GREEN),
                    );
                }
            }
        }
        if editor.editing && response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let (min, max) = editor.map.world_bounds(rect);
                let mut c = Coord::new(
                    min.lon
                        + (max.lon - min.lon) * (pos.x - rect.left()) as f64 / rect.width() as f64,
                    max.lat
                        - (max.lat - min.lat) * (pos.y - rect.top()) as f64 / rect.height() as f64,
                );
                // Exact shared vertex, not a fabricated connector.
                if let Some(n) = net
                    .graph
                    .node_weights()
                    .filter(|n| {
                        editor
                            .map
                            .to_screen(rect, n.coord.lat, n.coord.lon)
                            .distance(pos)
                            < 8.
                    })
                    .min_by(|a, b| {
                        editor
                            .map
                            .to_screen(rect, a.coord.lat, a.coord.lon)
                            .distance(pos)
                            .total_cmp(
                                &editor
                                    .map
                                    .to_screen(rect, b.coord.lat, b.coord.lon)
                                    .distance(pos),
                            )
                    })
                {
                    c = n.coord;
                }
                if let Some(i) = editor.vertex.take() {
                    editor.undo.push(selection.candidate.points.clone());
                    let was_closed =
                        selection.candidate.points.first() == selection.candidate.points.last();
                    if i < selection.candidate.points.len() {
                        selection.candidate.points[i] = c;
                        if was_closed && i == 0 {
                            let last = selection.candidate.points.len() - 1;
                            selection.candidate.points[last] = c;
                        }
                    }
                    changed = true;
                } else if let Some(i) = selection
                    .candidate
                    .points
                    .iter()
                    .position(|p| editor.map.to_screen(rect, p.lat, p.lon).distance(pos) < 8.)
                {
                    editor.vertex = Some(i);
                } else {
                    editor.undo.push(selection.candidate.points.clone());
                    selection.candidate.points.push(c);
                    changed = true;
                }
            }
        }
        editor.map.interact(ui, rect);
        if changed && editor.editing {
            selection.confirmed = false;
        }
        if let Ok(p) = normalize(&selection.candidate.points) {
            if let Ok(r) = route_planner::itinerary::polyline(&p) {
                ui.label(format!(
                    "周长 {:.1} 米 · 方向：逆时针 · ID {}",
                    r.length_m, selection.candidate.id
                ));
            }
        }
        if !selection.candidate.counterclockwise_allowed {
            ui.colored_label(Color32::RED, "该跑道的通行限制不允许逆时针");
        }
        if ui
            .add_enabled(
                !editor.editing && selection.candidate.counterclockwise_allowed,
                egui::Button::new("确认此跑道属于当前校区"),
            )
            .clicked()
        {
            match normalize(&selection.candidate.points) {
                Ok(p) => {
                    selection.candidate.points = p;
                    selection.context = key;
                    selection.geometry = crate::track::selection::geometry(&selection.candidate);
                    selection.confirmed = true;
                    editor.status = "跑道已确认".into();
                    changed = true;
                }
                Err(e) => editor.status = e,
            }
        }
        if !selection.confirmed {
            ui.label("未确认：正式生成将停止。");
        }
        ui.label(&editor.status);
        if changed {
            if let Err(e) = crate::api::model::save_config(&self.config) {
                self.run_page.track_editor.status = e;
            }
        }
    }
}
