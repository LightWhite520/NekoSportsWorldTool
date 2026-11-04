use super::App;

impl App {
    pub(super) fn login_backup_controls(&mut self, ui: &mut egui::Ui) {
        let idle = !self.backup_busy
            && self.active_jobs.load(std::sync::atomic::Ordering::Acquire) == 0
            && !self.any_busy()
            && !self.update.checking
            && !self.update.downloading;
        ui.collapsing("登录备份（加密 ZIP）", |ui| {
            ui.label("归档密码（至少 12 个字符；建议使用独立随机密码）");
            super::mobile::text_edit(ui, "archive_password", &mut self.archive_password,
                crate::platform::InputKind::Password, ui.available_width().min(280.0));
            ui.label("导出时再次确认密码");
            super::mobile::text_edit(ui, "archive_confirm", &mut self.archive_confirm,
                crate::platform::InputKind::Password, ui.available_width().min(280.0));
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        idle && self.session.is_some() && self.archive_password.chars().count() >= 12
                            && self.archive_password == self.archive_confirm,
                        egui::Button::new("导出登录 ZIP"),
                    )
                    .clicked()
                {
                    let result = crate::login_archive::export(
                        self.session.as_ref().unwrap(),
                        &self.identity,
                        &self.config,
                        &self.archive_password,
                    );
                    match result {
                        Ok(bytes) => {
                            self.archive_password.clear();
                            self.archive_confirm.clear();
                            self.export_login_zip(bytes);
                        },
                        Err(e) => self.status = e,
                    }
                }
                if ui
                    .add_enabled(idle && !self.archive_password.is_empty(), egui::Button::new("导入登录 ZIP"))
                    .clicked()
                {
                    self.import_login_zip();
                }
            });
            ui.small("ZIP 使用 AES-256 加密；导入将替换当前账号和设备身份，其他设置保留。密码不会保存，遗失后无法恢复。");
        });
    }

    fn export_login_zip(&mut self, bytes: Vec<u8>) {
        #[cfg(target_os = "android")]
        {
            self.backup_busy = crate::android::login_archive_picker(Some(&bytes));
            self.status = if self.backup_busy {
                "请选择 ZIP 保存位置"
            } else {
                "无法打开文件保存窗口"
            }
            .into();
        }
        #[cfg(not(target_os = "android"))]
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("ZIP", &["zip"])
            .set_file_name("neko-login.zip")
            .save_file()
        {
            self.status = match std::fs::write(path, bytes) {
                Ok(()) => "登录信息已导出".into(),
                Err(e) => format!("导出失败：{e}"),
            };
        }
    }

    fn import_login_zip(&mut self) {
        #[cfg(target_os = "android")]
        {
            self.backup_busy = crate::android::login_archive_picker(None);
            self.status = if self.backup_busy {
                "请选择登录 ZIP"
            } else {
                "无法打开文件选择窗口"
            }
            .into();
        }
        #[cfg(not(target_os = "android"))]
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("ZIP", &["zip"])
                .pick_file()
            {
                use std::io::Read;
                let result = (|| {
                    let mut bytes = Vec::new();
                    std::fs::File::open(path)?
                        .take(crate::login_archive::LIMIT + 1)
                        .read_to_end(&mut bytes)?;
                    Ok::<_, std::io::Error>(bytes)
                })();
                match result {
                    Ok(bytes) => {
                        if rfd::MessageDialog::new()
                            .set_title("导入登录信息")
                            .set_description("将替换当前账号、登录凭证和设备身份，其他设置保留。")
                            .set_buttons(rfd::MessageButtons::OkCancel)
                            .show()
                            == rfd::MessageDialogResult::Ok
                        {
                            self.apply_login_zip(&bytes);
                        }
                    }
                    Err(e) => self.status = format!("读取失败：{e}"),
                }
            }
            self.archive_password.clear();
            self.archive_confirm.clear();
        }
    }

    pub(super) fn poll_login_backup(&mut self) {
        #[cfg(target_os = "android")]
        if let Some(result) = crate::android::take_login_archive() {
            self.backup_busy = false;
            match result {
                Ok(Some(bytes)) => self.apply_login_zip(&bytes),
                Ok(None) => self.status = "登录信息已导出".into(),
                Err(error) => self.status = error,
            }
            self.archive_password.clear();
            self.archive_confirm.clear();
        }
    }

    fn apply_login_zip(&mut self, bytes: &[u8]) {
        let passphrase = std::mem::take(&mut self.archive_password);
        self.archive_confirm.clear();
        if self.active_jobs.load(std::sync::atomic::Ordering::Acquire) != 0
            || self.any_busy()
            || self.update.checking
            || self.update.downloading
        {
            self.status = "请等待当前任务结束后重新导入".into();
            return;
        }
        match crate::login_archive::import(bytes, &self.config, &passphrase) {
            Ok((b, config)) => {
                self.renew_session_jobs();
                self.begin_network_load();
                self.osm_page.busy = false;
                self.auth_expired = false;
                // Discard replies from the old account before refreshing the new one.
                (self.tx, self.rx) = std::sync::mpsc::channel();
                #[cfg(target_os = "android")]
                {
                    crate::android::complete_device_info();
                    let _ = crate::android::take_device_info();
                }
                self.identity = b.identity;
                self.device_buf = self.identity.clone();
                self.device_page = Default::default();
                self.log = Default::default();
                self.popup = None;
                self.ai_confirm = None;
                self.session = Some(b.session);
                self.config = config;
                self.username = self.config.username.clone();
                self.password = self.config.password.clone();
                self.remember = self.config.remember;
                self.records_page = Default::default();
                self.data_page = Default::default();
                self.user_page = Default::default();
                self.ai_page = super::ai::AiPage {
                    days: 1,
                    per_day: 1,
                    ..Default::default()
                };
                self.run_page.plan = None;
                self.run_page.preview = None;
                self.run_page.preview_stale = true;
                self.records_busy = false;
                self.data_busy = false;
                self.user_busy = false;
                self.refresh_data_page();
                self.refresh_records();
                self.refresh_user_page();
                self.refresh_ai_list();
                self.refresh_points();
                self.status = "登录信息已导入；凭证是否有效以服务器校验为准".into();
            }
            Err(e) => self.status = format!("导入失败：{e}"),
        }
    }
}
