//! Settings page: the overlay's own settings (`overlay.toml`), the runtime switches and the
//! deployed `poe-hook.toml`, read-only.

use std::sync::atomic::Ordering;

use imgui::Ui;

use super::{settings, widgets};

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
            // 0 (automatic) shows as the scale in use once a frame has been drawn.
        }
    }

    pub fn draw(&mut self, ui: &Ui) {
        let cfg = crate::config();
        let read_only = crate::host::read_only();
        widgets::header(
            ui,
            "The overlay's own settings and the deployed config.",
            "Saved in logs/poe-hook/overlay.toml, which a deploy never overwrites; Movement log and \
             Position sync last this session only. poe-hook.toml is written by deploys: edit it, \
             then Reload core.",
        );
        ui.separator_with_text("General");
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
        let in_use = crate::overlay::wanted_scale(crate::overlay::extent().1);
        if self.scale <= 0.0 {
            self.scale = in_use;
        }
        let over = settings::scale_override();
        let auto = over.unwrap_or(cfg.overlay.scale) <= 0.0;
        widgets::label(ui, "Scale");
        let mut auto_on = auto;
        if ui.checkbox("Automatic##scale", &mut auto_on) {
            // Some(0.0) = automatic, also over a fixed overlay.scale in poe-hook.toml.
            let v = if auto_on { 0.0 } else { in_use };
            settings::update(|s| s.scale_override = Some(v));
            self.scale = in_use;
        }
        if ui.is_item_hovered() {
            ui.tooltip_text("Display height / 1080 in quarter steps (1.25 at 1440p).");
        }
        ui.same_line();
        {
            let _d = ui.begin_disabled_with_cond(auto);
            ui.set_next_item_width(12.0 * ui.current_font_size());
            let format = imgui::NumericFormat::new("%.2f").expect("valid format");
            ui.slider_config("##scale", 0.5f32, 4.0f32)
                .display_format(format)
                .build(&mut self.scale);
            if ui.is_item_deactivated_after_edit() {
                let v = ((self.scale * 4.0).round() / 4.0).clamp(0.5, 4.0);
                self.scale = v;
                settings::update(|s| s.scale_override = Some(v));
            }
        }
        if auto {
            self.scale = in_use;
        }
        ui.same_line();
        ui.text_disabled(match (auto, over) {
            (true, _) => format!("{in_use:.2} now"),
            (false, Some(_)) => format!("fixed {in_use:.2} (overlay.toml)"),
            (false, None) => format!("fixed {in_use:.2} (overlay.scale of poe-hook.toml)"),
        });
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
        "Read-only (official mode or any untrusted connection): no crypto bypass, no position \
         sync, no test input, no memory writes. The overlay only reads memory and captures \
         traffic.",
    );
}
