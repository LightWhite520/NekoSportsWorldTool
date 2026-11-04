//! Offline native rendering fixture. It has no account loader or submission client.
use super::{map::MapState, run_review::draw_preview};
use crate::track::review::RunReview;
use eframe::egui;

fn demo_data(variation: f64, drift_variation: f64) -> Result<RunReview, String> {
    let mut points = Vec::new();
    for (cy, first) in [(40.0, 0.0), (-40.0, std::f64::consts::PI)] {
        for i in 0..=60 {
            let angle = first + i as f64 / 60.0 * std::f64::consts::PI;
            points.push(route_planner::Coord::new(
                10.0 + 36.0 * angle.cos() / (111320.0 * 30_f64.to_radians().cos()),
                30.0 + (cy + 36.0 * angle.sin()) / 111132.0,
            ));
        }
    }
    points.push(points[0]);
    let trip = route_planner::itinerary::Itinerary::looping(route_planner::itinerary::polyline(&points)?);
    let route = route_planner::lanes::expand(&trip, 2090.0, 7, &[])?;
    let mut track = crate::track::motion::build_with_variation(
        &route,
        840,
        7,
        1_700_000_000_000,
        &[],
        true,
        variation,
        drift_variation,
    )?;
    crate::track::altitude::override_bd_a_range(
        &mut track,
        crate::track::altitude::AltitudeRange {
            min_m: 11.6,
            max_m: 22.8,
        },
    )?;
    RunReview::from_track(&track)
}

pub(crate) fn run(
    capture_dir: Option<std::path::PathBuf>,
    variation: f64,
    drift_variation: f64,
) -> Result<(), String> {
    let data = demo_data(variation, drift_variation)?;
    if let Some(path) = &capture_dir {
        std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
        std::fs::write(
            path.join("generated-review.json"),
            serde_json::to_vec_pretty(&data).unwrap(),
        )
        .map_err(|e| e.to_string())?;
    }
    eframe::run_native(
        "生成数据预览 · 离线示例",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([880.0, 900.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            super::fonts::install(&cc.egui_ctx);
            super::theme::apply(&cc.egui_ctx);
            Ok(Box::new(Demo {
                data,
                variation: variation as f32,
                drift_variation: drift_variation as f32,
                map: MapState::default(),
                map_size: egui::Vec2::ZERO,
                capture_dir,
                stage: 0,
                frames: 0,
                feedback: String::new(),
            }))
        }),
    )
    .map_err(|e| e.to_string())
}

struct Demo {
    data: RunReview,
    variation: f32,
    drift_variation: f32,
    map: MapState,
    map_size: egui::Vec2,
    capture_dir: Option<std::path::PathBuf>,
    stage: usize,
    frames: usize,
    feedback: String,
}

impl eframe::App for Demo {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(60));
        if let Some(directory) = &self.capture_dir {
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let pixels: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    let output = image::RgbaImage::from_raw(
                        image.width() as u32,
                        image.height() as u32,
                        pixels,
                    )
                    .unwrap();
                    output
                        .save(directory.join(if self.stage == 0 {
                            "preview-top.png"
                        } else {
                            "preview-bottom.png"
                        }))
                        .unwrap();
                    self.stage += 1;
                    self.frames = 0;
                    if self.stage >= 2 {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("离线合成跑道 · 单道宽 1.22 米 · 按钮仅演示，不会提交记录");
            let previous = (self.variation, self.drift_variation);
            super::run::draw_motion_variation(ui, &mut self.variation);
            super::run::draw_drift_variation(ui, &mut self.drift_variation);
            if previous != (self.variation, self.drift_variation) {
                match demo_data(self.variation as f64, self.drift_variation as f64) {
                    Ok(data) => {
                        self.data = data;
                        self.map_size = egui::Vec2::ZERO;
                        self.feedback.clear();
                    }
                    Err(error) => self.feedback = error,
                }
            }
            if !self.feedback.is_empty() {
                ui.label(&self.feedback);
            }
            let mut scroll = egui::ScrollArea::vertical();
            if self.capture_dir.is_some() {
                scroll = scroll.vertical_scroll_offset(if self.stage == 0 { 0.0 } else { 650.0 });
            }
            scroll.show(ui, |ui| {
                if let Some(approved) =
                    draw_preview(ui, &self.data, &mut self.map, &mut self.map_size)
                {
                    self.feedback = if approved {
                        "已选择确认（演示模式，未提交）"
                    } else {
                        "已选择取消（演示模式）"
                    }
                    .into();
                }
            });
        });
        self.frames += 1;
        if self.capture_dir.is_some() && self.frames == 8 && self.stage < 2 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
        }
    }
}
