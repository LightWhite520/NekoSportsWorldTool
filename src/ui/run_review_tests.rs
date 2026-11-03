use super::*;
use crate::track::review::RunReview;
use std::sync::mpsc;

fn ready(ctx: &egui::Context) -> (App, mpsc::Receiver<bool>) {
    let mut app = App::from_state(
        ctx,
        HeaderIdentity::default(),
        Session {
            uid: 1,
            token: "offline-fixture".into(),
            unid: "1".into(),
            ..Default::default()
        },
        Config::default(),
    );
    let mut track = crate::track::generator::build(
        1200.0,
        600,
        7,
        (30.0, 10.0),
        0,
        &[(30.0, 10.0), (30.001, 10.001)],
    )
    .unwrap();
    crate::track::altitude::override_bd_a_range(
        &mut track,
        crate::track::altitude::AltitudeRange {
            min_m: 11.6,
            max_m: 22.8,
        },
    )
    .unwrap();
    let (decision, response) = mpsc::channel();
    app.run_busy = true;
    app.run_review = Some(crate::ui::run_review::Request {
        epoch: app.session_epoch,
        control: app.run_control.clone(),
        data: RunReview::from_track(&track).unwrap(),
        decision,
        map: map::MapState::default(),
        map_size: Vec2::ZERO,
    });
    (app, response)
}

#[test]
fn preview_displays_all_five_sections_in_requested_order() {
    let ctx = egui::Context::default();
    let (mut app, response) = ready(&ctx);
    for _ in 0..3 {
        frame(&mut app, &ctx, 880.0, 1900.0, vec![]);
    }
    let output = frame(&mut app, &ctx, 880.0, 1900.0, vec![]);
    let labels = ["路线图", "配速", "步频", "步幅", "海拔", "确认提交这份数据"];
    let positions: Vec<_> = labels
        .iter()
        .map(|label| label_position(&output, label).unwrap_or_else(|| panic!("missing {label}")))
        .collect();
    assert!(positions.windows(2).all(|p| p[0].y < p[1].y));
    assert!(matches!(
        response.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(app.run_control.cancellable());
}

#[test]
fn phone_preview_buttons_require_a_click_and_only_send_one_decision() {
    for (w, h, approve) in [
        (320.0, 480.0, false),
        (360.0, 640.0, true),
        (880.0, 900.0, true),
    ] {
        let ctx = egui::Context::default();
        let (mut app, response) = ready(&ctx);
        let label = if approve {
            "确认提交这份数据"
        } else {
            "取消，返回调整"
        };
        let at = scroll_to_label(&mut app, &ctx, w, h, label);
        assert!(matches!(
            response.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        click(&mut app, &ctx, w, h, at);
        assert_eq!(response.try_recv().unwrap(), approve);
        assert!(app.run_review.is_none());
        click(&mut app, &ctx, w, h, at);
        assert!(response.try_recv().is_err());
        assert_eq!(app.active_jobs.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}

#[test]
fn stale_or_cancelled_generation_is_never_presented_for_confirmation() {
    for stale_epoch in [false, true] {
        let ctx = egui::Context::default();
        let (mut app, response) = ready(&ctx);
        let request = app.run_review.take().unwrap();
        if stale_epoch {
            app.session_epoch += 1;
        } else {
            app.run_control.cancel();
        }
        app.run_review_tx.send(request).unwrap();
        app.poll_run_reviews();
        assert!(app.run_review.is_none());
        assert!(matches!(
            response.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}
