use super::*;
use crate::api::model::{Config, HeaderIdentity, Session};
use egui::{Event, PointerButton, Pos2, Rect};

struct TestDir(std::path::PathBuf);
impl TestDir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("track-editor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        crate::platform::TEST_DATA_DIR.with(|v| *v.borrow_mut() = Some(p.clone()));
        Self(p)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        crate::platform::TEST_DATA_DIR.with(|v| *v.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn draw(
    app: &mut App,
    ctx: &egui::Context,
    net: &RoadGraph,
    inputs: &PreviewInputs,
    width: f32,
    events: Vec<Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 480.))),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| app.draw_track_editor(ui, net, inputs));
            });
        },
    )
}
fn click_label(
    app: &mut App,
    ctx: &egui::Context,
    net: &RoadGraph,
    inputs: &PreviewInputs,
    width: f32,
    label: &str,
) {
    for _ in 0..80 {
        let output = draw(app, ctx, net, inputs, width, vec![]);
        let pos = output.shapes.iter().find_map(|c| {
            if let egui::Shape::Text(t) = &c.shape {
                let r = Rect::from_min_size(t.pos, t.galley.size());
                (t.galley.text() == label && c.clip_rect.contains(r.center())).then_some(r.center())
            } else {
                None
            }
        });
        if let Some(pos) = pos {
            for pressed in [true, false] {
                draw(
                    app,
                    ctx,
                    net,
                    inputs,
                    width,
                    vec![
                        Event::PointerMoved(pos),
                        Event::PointerButton {
                            pos,
                            button: PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
            return;
        }
        draw(
            app,
            ctx,
            net,
            inputs,
            width,
            vec![
                Event::PointerMoved(Pos2::new(width / 2., 450.)),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0., -70.),
                    modifiers: Default::default(),
                },
            ],
        );
        for _ in 0..12 {
            draw(app, ctx, net, inputs, width, vec![]);
        }
    }
    panic!("control inaccessible: {label} at {width}");
}
#[test]
fn phone_confirmation_is_operable_and_expires_after_checkpoint_refresh() {
    let _dir = TestDir::new();
    let xml=br#"<osm><node id="1" lat="30" lon="10"/><node id="2" lat="30" lon="10.001"/><node id="3" lat="30.001" lon="10"/><way id="1"><nd ref="1"/><nd ref="2"/><nd ref="3"/><nd ref="1"/><tag k="leisure" v="track"/></way></osm>"#;
    let net = route_planner::load_osm(xml).unwrap();
    for width in [320., 412., 880.] {
        let ctx = egui::Context::default();
        let mut config = Config::default();
        config.track_enabled = true;
        let mut inputs = PreviewInputs {
            uid: 0,
            unid: "fixture".into(),
            anchor: crate::location::Coordinate::new(30., 10., 2.).unwrap(),
            points: vec![(30., 10.)],
            fences: vec![],
            loaded_ms: 1,
        };
        config.track_selection = Some(TrackSelection {
            candidate: net.tracks[0].clone(),
            context: context(&net, &inputs.unid, &[], &inputs.points),
            confirmed: false,
            geometry: String::new(),
        });
        let mut app = App::from_state(&ctx, HeaderIdentity::default(), Session::default(), config);
        for _ in 0..2 {
            draw(&mut app, &ctx, &net, &inputs, width, vec![]);
        }
        click_label(
            &mut app,
            &ctx,
            &net,
            &inputs,
            width,
            "确认此跑道属于当前校区",
        );
        assert!(app
            .config
            .track_selection
            .as_ref()
            .unwrap()
            .resolve(&net, &inputs.unid, &[], &inputs.points)
            .is_ok());
        inputs.points.push((30.0001, 10.));
        inputs.loaded_ms = 2;
        draw(&mut app, &ctx, &net, &inputs, width, vec![]);
        assert!(!app.config.track_selection.as_ref().unwrap().confirmed);
    }
}
