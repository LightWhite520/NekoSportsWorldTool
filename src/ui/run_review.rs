//! Confirmation page for the immutable final run data.
use super::{map::MapState, mobile, theme, App};
use crate::task_control::Control;
use crate::track::review::{format_pace, Point, RunReview};
use eframe::egui;
use std::sync::{mpsc, Arc};

pub(super) struct Request {
    pub epoch: u64,
    pub control: Arc<Control>,
    pub data: RunReview,
    pub decision: mpsc::Sender<bool>,
    pub map: MapState,
    pub map_size: egui::Vec2,
}

pub(super) fn wait_for_decision(
    rx: mpsc::Receiver<bool>,
    control: &Control,
) -> Result<bool, String> {
    loop {
        control.check()?;
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(decision) => {
                control.check()?;
                return Ok(decision);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(false),
        }
    }
}

impl App {
    pub(super) fn poll_run_reviews(&mut self) {
        while let Ok(request) = self.run_review_rx.try_recv() {
            if request.epoch == self.session_epoch
                && self.run_busy
                && self.session.is_some()
                && Arc::ptr_eq(&request.control, &self.run_control)
                && request.control.check().is_ok()
            {
                self.run_review = Some(request);
                self.tab = 0;
                self.status = "数据已生成，请查看后决定是否提交".into();
            }
            // Dropping a stale request also closes its decision channel.
        }
    }

    pub(super) fn draw_run_review(&mut self, ui: &mut egui::Ui) {
        let Some(request) = self.run_review.as_mut() else {
            return;
        };
        let decision = draw_preview(ui, &request.data, &mut request.map, &mut request.map_size);
        if let Some(approved) = decision {
            if !approved {
                self.run_control.cancel();
            }
            if let Some(request) = self.run_review.take() {
                if request.decision.send(approved).is_err() {
                    self.status = "本次生成已失效，请重新生成".into();
                    return;
                }
            }
            self.status = if approved {
                "正在提交已确认的数据…"
            } else {
                "已取消，等待任务结束…"
            }
            .into();
        }
    }
}

pub(super) fn draw_preview(
    ui: &mut egui::Ui,
    review: &RunReview,
    map: &mut MapState,
    map_size: &mut egui::Vec2,
) -> Option<bool> {
    ui.heading("提交前预览");
    ui.label("以下是本次生成的完整数据，确认后将提交这一份结果。");
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(format!("{:.2} 公里", review.distance_m / 1000.0))
                .strong()
                .color(ui.visuals().text_color()),
        );
        ui.label(format!(
            "用时 {}:{:02}",
            review.duration_s / 60,
            review.duration_s % 60
        ));
        ui.label(format!("{} 步", review.steps));
        ui.label(format!("累计爬升 {:.2} 米", review.ascent_m));
    });
    ui.label(format!(
        "开始时间：{}",
        crate::track::geom::fmt_cst(review.start_ms, "%Y-%m-%d %H:%M:%S")
    ));
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new("路线图")
            .strong()
            .color(ui.visuals().text_color()),
    );
    let (response, painter) =
        ui.allocate_painter(egui::vec2(ui.available_width(), 250.0), egui::Sense::drag());
    let rect = response.rect;
    if *map_size != rect.size() {
        map.fit(rect.shrink(15.0), &review.route);
        *map_size = rect.size();
    }
    painter.rect_filled(rect, 5.0, ui.visuals().extreme_bg_color);
    let painter = painter.with_clip_rect(rect);
    map.draw_polyline(&painter, rect, &review.route, theme::ok(), 1.5);
    if let Some(&(lat, lon)) = review.route.first() {
        map.draw_point(&painter, rect, lat, lon, theme::ok(), 5.0);
        painter.text(
            map.to_screen(rect, lat, lon),
            egui::Align2::LEFT_BOTTOM,
            "起点",
            egui::FontId::proportional(12.0),
            ui.visuals().text_color(),
        );
    }
    if let Some(&(lat, lon)) = review.route.last() {
        map.draw_point(&painter, rect, lat, lon, theme::warn(), 5.0);
        painter.text(
            map.to_screen(rect, lat, lon),
            egui::Align2::RIGHT_TOP,
            "终点",
            egui::FontId::proportional(12.0),
            ui.visuals().text_color(),
        );
    }
    map.interact(ui, rect);
    if ui.small_button("适应完整路线").clicked() {
        *map_size = egui::Vec2::ZERO;
    }
    ui.label("配速来自轨迹点；步频和步幅按实际 10 秒区间计算，末段使用实际时长。");
    chart(
        ui,
        "配速",
        &format!("平均 {} /公里", format_pace(review.avg_pace_s)),
        &review.pace,
        review.duration_s,
        true,
        "/公里",
    );
    chart(
        ui,
        "步频",
        &format!("平均 {:.0} 步/分钟", review.avg_cadence),
        &review.cadence,
        review.duration_s,
        false,
        "步/分钟",
    );
    let stride = review
        .avg_stride_cm
        .map(|v| format!("平均 {v:.1} 厘米/步"))
        .unwrap_or_else(|| "暂无步幅".into());
    chart(
        ui,
        "步幅",
        &stride,
        &review.stride,
        review.duration_s,
        false,
        "厘米/步",
    );
    chart(
        ui,
        "海拔",
        &format!("累计爬升 {:.2} 米", review.ascent_m),
        &review.altitude,
        review.duration_s,
        false,
        "米",
    );
    ui.add_space(10.0);
    let mut decision = None;
    mobile::row(ui, |ui| {
        if ui.add(theme::primary_btn("确认提交这份数据")).clicked() {
            decision = Some(true);
        }
        if ui.button("取消，返回调整").clicked() {
            decision = Some(false);
        }
    });
    decision
}

fn chart(
    ui: &mut egui::Ui,
    title: &str,
    summary: &str,
    points: &[Point],
    duration: i64,
    pace: bool,
    unit: &str,
) {
    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(title)
                .strong()
                .color(ui.visuals().text_color()),
        );
        ui.label(summary);
    });
    let (response, painter) = ui.allocate_painter(
        egui::vec2(ui.available_width(), 178.0),
        egui::Sense::hover(),
    );
    let rect = response.rect;
    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(58.0, 8.0),
        rect.max - egui::vec2(10.0, 25.0),
    );
    let values = points
        .iter()
        .filter_map(|p| p.value)
        .filter(|v| v.is_finite());
    let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
    for value in values {
        min = min.min(value);
        max = max.max(value);
    }
    if !min.is_finite() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "暂无数据",
            egui::FontId::proportional(13.0),
            ui.visuals().text_color(),
        );
        return;
    }
    let padding = ((max - min) * 0.08).max(if pace { 1.0 } else { 0.1 });
    min -= padding;
    max += padding;
    let color = ui.visuals().text_color();
    let position = |time: f64, value: f64| {
        let y = ((value - min) / (max - min)) as f32;
        egui::pos2(
            plot.left() + plot.width() * (time / duration.max(1) as f64) as f32,
            if pace {
                plot.top() + y * plot.height()
            } else {
                plot.bottom() - y * plot.height()
            },
        )
    };
    for fraction in [0.0, 0.5, 1.0] {
        let value = min + fraction * (max - min);
        let pos = position(0.0, value);
        painter.line_segment(
            [pos, egui::pos2(plot.right(), pos.y)],
            egui::Stroke::new(0.5_f32, ui.visuals().weak_text_color()),
        );
        let label = if pace {
            format_pace(value)
        } else {
            format!("{value:.1}")
        };
        painter.text(
            egui::pos2(plot.left() - 5.0, pos.y),
            egui::Align2::RIGHT_CENTER,
            label,
            egui::FontId::proportional(11.0),
            color,
        );
    }
    let mut line = Vec::new();
    let stroke = egui::Stroke::new(1.5_f32, theme::ok());
    for point in points {
        if let Some(value) = point.value.filter(|v| v.is_finite()) {
            line.push(position(point.time_s, value));
        } else if !line.is_empty() {
            painter.add(egui::Shape::line(std::mem::take(&mut line), stroke));
        }
    }
    if !line.is_empty() {
        painter.add(egui::Shape::line(line, stroke));
    }
    for (x, t, align) in [
        (plot.left(), 0, egui::Align2::LEFT_TOP),
        (plot.center().x, duration / 2, egui::Align2::CENTER_TOP),
        (plot.right(), duration, egui::Align2::RIGHT_TOP),
    ] {
        painter.text(
            egui::pos2(x, plot.bottom() + 6.0),
            align,
            format!("{}:{:02}", t / 60, t % 60),
            egui::FontId::proportional(11.0),
            color,
        );
    }
    if let Some(pointer) = response.hover_pos().filter(|p| plot.contains(*p)) {
        let time = ((pointer.x - plot.left()) / plot.width()) as f64 * duration as f64;
        let index = points
            .partition_point(|p| p.time_s < time)
            .min(points.len().saturating_sub(1));
        if let Some(point) = points.get(index) {
            let value = point
                .value
                .map(|v| {
                    if pace {
                        format_pace(v)
                    } else {
                        format!("{v:.2}")
                    }
                })
                .unwrap_or_else(|| "—".into());
            response.on_hover_text(format!(
                "{}:{:02}  {value} {unit}",
                point.time_s as i64 / 60,
                point.time_s as i64 % 60
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_or_cancelled_review_never_approves() {
        let (tx, rx) = mpsc::channel();
        drop(tx);
        assert!(!wait_for_decision(rx, &Control::default()).unwrap());
        let (tx, rx) = mpsc::channel();
        tx.send(true).unwrap();
        let control = Control::default();
        control.cancel();
        assert!(wait_for_decision(rx, &control).is_err());
    }
}
