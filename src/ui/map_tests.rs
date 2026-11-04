use super::*;
use egui::{Event, PointerButton};

struct Harness {
    ctx: egui::Context,
    map: MapState,
    canvas: Rect,
    scroll: Vec2,
    time: f64,
}

impl Harness {
    fn new() -> Self {
        let mut harness = Self {
            ctx: egui::Context::default(),
            map: MapState::default(),
            canvas: Rect::NOTHING,
            scroll: Vec2::ZERO,
            time: 0.0,
        };
        for _ in 0..3 {
            harness.frame(vec![]);
        }
        harness
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.time += 1.0 / 60.0;
        let _ = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400., 320.))),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let area =
                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                ui.add_space(70.);
                                let (response, _) =
                                    ui.allocate_painter(Vec2::new(220., 160.), egui::Sense::drag());
                                self.canvas = response.rect;
                                self.map.interact(ui, &response);
                                ui.add_space(600.);
                            });
                    self.scroll = area.state.offset;
                });
            },
        );
    }

    fn wheel(&mut self, pos: Pos2) {
        self.frame(vec![
            Event::PointerMoved(pos),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: Vec2::new(0., -3.),
                modifiers: Default::default(),
            },
        ]);
        for _ in 0..30 {
            self.frame(vec![]);
        }
    }

    fn pointer(&mut self, pos: Pos2, pressed: Option<bool>, touch: bool) {
        let mut events = vec![Event::PointerMoved(pos)];
        if let Some(pressed) = pressed {
            events.push(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        if touch {
            events.push(Event::Touch {
                device_id: egui::TouchDeviceId(0),
                id: egui::TouchId(0),
                phase: match pressed {
                    Some(true) => egui::TouchPhase::Start,
                    Some(false) => egui::TouchPhase::End,
                    None => egui::TouchPhase::Move,
                },
                pos,
                force: None,
            });
        }
        self.frame(events);
    }
}

#[test]
fn wheel_over_map_zooms_without_scrolling_page_including_smoothing_tail() {
    let mut h = Harness::new();
    let scale = h.map.scale;
    h.wheel(h.canvas.center());
    assert!(h.map.scale < scale);
    assert_eq!(h.scroll, Vec2::ZERO);

    let scale = h.map.scale;
    h.wheel(Pos2::new(300., 150.));
    assert_eq!(h.map.scale, scale);
    assert!(h.scroll.y > 0.);
}

#[test]
fn map_drag_keeps_ownership_when_pointer_leaves_canvas() {
    for touch in [false, true] {
        let mut h = Harness::new();
        let start = h.canvas.center();
        h.pointer(start, Some(true), touch);
        h.pointer(start + Vec2::new(10., -20.), None, touch);
        let offset = h.map.offset;
        assert!(h.map.dragged);
        assert!(offset.y < 0.);
        h.pointer(Pos2::new(300., 100.), None, touch);
        assert!(h.map.dragged);
        assert_ne!(h.map.offset, offset);
        assert_eq!(h.scroll, Vec2::ZERO);
        h.pointer(Pos2::new(300., 100.), Some(false), touch);
    }
}

#[test]
fn page_drag_entering_map_does_not_pan_map() {
    for touch in [false, true] {
        let mut h = Harness::new();
        h.pointer(Pos2::new(300., 190.), Some(true), touch);
        h.pointer(Pos2::new(100., 150.), None, touch);
        assert!(h.canvas.contains(Pos2::new(100., 150.)));
        assert!(h.scroll.y > 0.);
        assert!(!h.map.dragged);
        assert_eq!(h.map.offset, Vec2::ZERO);
        h.pointer(Pos2::new(100., 150.), Some(false), touch);
    }
}
