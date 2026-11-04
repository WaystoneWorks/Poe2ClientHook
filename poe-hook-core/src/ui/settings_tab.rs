//! Settings page: the overlay's own settings (`overlay.toml`), the runtime switches and the
//! deployed `poe-hook.toml`, read-only.

use std::sync::atomic::Ordering;

use imgui::Ui;

use super::settings;

pub struct SettingsView {
    /// The scale slider's value while it is edited.
    scale: f32,
}

impl SettingsView {
    pub fn new() -> Self {
        Self {
            scale: settings::get()
                .scale_override
                .unwrap_or(crate::config().overlay.scale),
        }
    }

    pub fn draw(&mut self, ui: &Ui) {
        let cfg = crate::config();
        let read_only = crate::host::read_only();
        ui.text("General");
        let mut popups = settings::get().show_slow_hook_popups;
        if ui.checkbox("Show slow hook popups", &mut popups) {
            settings::update(|s| s.show_slow_hook_popups = popups);
        }
        ui.same_line();
        ui.text_disabled("(slow hooks are logged either way)");
        let mut movement = crate::dump_types::MOVEMENT_LOG.load(Ordering::Relaxed);
        if ui.checkbox(
            "Movement log (per-frame player position in the log)",
            &mut movement,
        ) {
            crate::dump_types::MOVEMENT_LOG.store(movement, Ordering::Relaxed);
        }
        if !read_only {
            let mut sync = crate::position_sync::ENABLED.load(Ordering::Relaxed);
            if ui.checkbox("Position sync (reports to our instance server)", &mut sync) {
                crate::position_sync::ENABLED.store(sync, Ordering::Relaxed);
            }
        }
        ui.set_next_item_width(200.0);
        ui.slider_config("Scale", 0.5f32, 3.0f32)
            .build(&mut self.scale);
        if ui.is_item_deactivated_after_edit() {
            let v = self.scale;
            settings::update(|s| s.scale_override = Some(v));
        }
        ui.same_line();
        if ui.button("Reset##scale") {
            self.scale = cfg.overlay.scale;
            settings::update(|s| s.scale_override = None);
        }
        ui.same_line();
        ui.text_disabled("overrides overlay.scale of poe-hook.toml; applies at the next start");
        ui.text_disabled("Saved in logs/poe-hook/overlay.toml.");
        ui.spacing();
        if ui.collapsing_header(
            "Deployed config (edit poe-hook.toml, then Reload core)",
            imgui::TreeNodeFlags::empty(),
        ) {
            draw_deployed(ui, cfg, read_only);
        }
    }
}

fn draw_deployed(ui: &Ui, cfg: &poe_hook_shared::config::Config, read_only: bool) {
    let f = &cfg.features;
    let rows = [
        ("mode", format!("{:?}", cfg.mode)),
        ("crash_log", f.crash_log.to_string()),
        (
            "crypto_bypass",
            format!(
                "{}{}",
                f.crypto_bypass,
                if read_only { " (off: read-only)" } else { "" }
            ),
        ),
        ("net_trace", f.net_trace.to_string()),
        ("block_external", f.block_external.to_string()),
        ("dump_object_types", f.dump_object_types.to_string()),
        ("position_sync_port", f.position_sync_port.to_string()),
        ("trace_addresses", format!("{:?}", f.trace_addresses)),
        ("packets.capture", cfg.packets.capture.to_string()),
        ("packets.redact", cfg.packets.redact.to_string()),
        ("packets.keep", cfg.packets.keep.to_string()),
        ("overlay.scale", cfg.overlay.scale.to_string()),
        ("reload.auto", cfg.reload.auto.to_string()),
    ];
    if let Some(_t) = ui.begin_table_header_with_flags(
        "features",
        [
            imgui::TableColumnSetup::new("setting"),
            imgui::TableColumnSetup::new("value"),
        ],
        crate::ui::table_opts(),
    ) {
        for (k, v) in rows {
            ui.table_next_column();
            ui.text(k);
            ui.table_next_column();
            ui.text(v);
        }
    }
    ui.separator();
    ui.text_wrapped(
        "Read-only (official mode or any external connection): no crypto bypass, no position \
         sync, no test input, no memory writes. The overlay only reads memory and captures \
         traffic.",
    );
}
