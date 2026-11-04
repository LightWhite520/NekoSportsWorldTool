use super::*;


#[test]
fn encrypted_archive_controls_are_reachable_in_compact_account_panel() {
    let (w, h) = (320.0, 480.0);
    let ctx = egui::Context::default();
    let mut app = App::from_state(&ctx, HeaderIdentity::default(), Session::default(), Config::default());
    frame(&mut app, &ctx, w, h, vec![]);
    let output = frame(&mut app, &ctx, w, h, vec![]);
    click(&mut app, &ctx, w, h, label_position(&output, "账号与登录").unwrap());
    for target in ["登录备份（加密 ZIP）", "导入登录 ZIP"] {
        let mut found = None;
        for _ in 0..100 {
            let output = frame(&mut app, &ctx, w, h, vec![]);
            if let Some(pos) = label_position(&output, target) { found = Some(pos); break; }
            frame(&mut app, &ctx, w, h, vec![
                Event::PointerMoved(Pos2::new(160.0, 110.0)),
                Event::MouseWheel { unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, -30.0), modifiers: Default::default() },
            ]);
        }
        let pos = found.unwrap_or_else(|| panic!("unreachable account control: {target}"));
        click(&mut app, &ctx, w, h, pos);
    }
    // Empty archive password keeps import disabled, so no file picker was opened.
    assert!(!app.backup_busy);
    assert!(app.archive_password.is_empty());
}
