//! Sync page (our server only): the instance server's simulation state for this session and
//! the client's position against the server's prediction.

use dear_implot::{ImPlotExt, LinePlot, Plot, PlotCond, PlotContext};
use imgui::Ui;

use super::widgets::{self, Tone};

pub struct ServerView {
    seconds: i32,
}

impl ServerView {
    pub fn new() -> Self {
        Self { seconds: 10 }
    }

    /// The Sync page: live chips, the server's state table and the desync analysis.
    pub fn draw(&mut self, ui: &Ui, plots: &PlotContext) {
        widgets::header(
            ui,
            "The instance server's simulation state against the client's position.",
            "Needs position sync and [dev] debug_link. The prediction is for the frame \
             the server's next message applies in, so about one frame of walking is expected.",
        );
        let Some((age, values)) = crate::devlink::state() else {
            widgets::empty(
                ui,
                "No state from the server yet (needs position sync and [dev] debug_link).",
            );
            return;
        };
        let linediv = values
            .iter()
            .find(|(k, _)| k.contains("linediv"))
            .map(|(_, v)| v.clone());
        let fresh = age.as_millis() < 2000;
        widgets::chip(
            ui,
            if fresh { "state live" } else { "state stale" },
            if fresh { Tone::Ok } else { Tone::Warn },
        );
        ui.same_line();
        ui.text(format!("last state {} ms ago", age.as_millis()));
        if let Some(l) = linediv {
            ui.same_line();
            ui.text(format!("linediv {l}"));
        }
        if ui.collapsing_header("Server state", imgui::TreeNodeFlags::empty())
            && let Some(_t) = ui.begin_table_header_with_flags(
                "server_state2",
                [
                    imgui::TableColumnSetup::new("name"),
                    imgui::TableColumnSetup::new("server value"),
                ],
                crate::ui::table_opts(),
            )
        {
            for (k, v) in &values {
                ui.table_next_column();
                ui.text(k);
                ui.table_next_column();
                ui.text(v);
            }
        }
        ui.separator();
        self.draw_desync(ui, plots);
    }

    fn draw_desync(&mut self, ui: &Ui, plots: &PlotContext) {
        widgets::label(ui, "Seconds");
        ui.set_next_item_width(crate::ui::widgets::px(ui, 200.0));
        ui.slider("##seconds_desync", 2, 120, &mut self.seconds);
        let now = poe_hook_shared::unix_ms();
        let since = now.saturating_sub(self.seconds as u64 * 1000);
        let samples = crate::devlink::samples(since);
        if samples.is_empty() {
            widgets::empty(ui, "No server states yet (our server, debug link).");
            return;
        }
        let t: Vec<f64> = samples
            .iter()
            .map(|s| (s.unix_ms as f64 - now as f64) / 1000.0)
            .collect();
        let dist = |f: fn(&crate::devlink::Sample) -> Option<(f32, f32)>| -> Vec<f64> {
            samples
                .iter()
                .map(|s| f(s).map_or(0.0, |p| ((p.0 - s.client.0).hypot(p.1 - s.client.1)) as f64))
                .collect()
        };
        let pred = dist(|s| s.predicted);
        let report = dist(|s| s.report);
        let over = pred
            .iter()
            .filter(|d| **d > crate::devlink::DESYNC_CELLS as f64)
            .count();
        let worst = pred.iter().cloned().fold(0.0, f64::max);
        ui.text_wrapped(format!(
            "{}; prediction off by more than {} cells in {}; worst {worst:.2} cells",
            widgets::plural(samples.len(), "sample"),
            crate::devlink::DESYNC_CELLS,
            widgets::plural(over, "frame"),
        ));
        ui.same_line();
        ui.text_disabled("(?)");
        if ui.is_item_hovered() {
            widgets::help_tooltip(
                ui,
                "The prediction is for the frame the server's next message applies in (one \
                 frame ahead), so about one frame of walking (~1.2 cells) is expected while \
                 moving.",
            );
        }
        self.delta_table(ui, &samples, &pred, now);
        let plot = ui.implot(plots);
        let width = ui.content_region_avail()[0];
        if let Some(token) = plot.begin_plot_with_size(
            "Distance from the client's position (cells)",
            [width, crate::ui::widgets::px(ui, 260.0)],
        ) {
            plot.setup_axes_limits(
                -(self.seconds as f64),
                0.0,
                0.0,
                (worst * 1.2).max(3.0),
                PlotCond::Always,
            );
            LinePlot::new("server prediction", &t, &pred).plot(&plot);
            LinePlot::new("last report", &t, &report).plot(&plot);
            token.end();
        }
        let xs: Vec<f64> = samples.iter().map(|s| s.client.0 as f64).collect();
        let pxs: Vec<f64> = samples
            .iter()
            .map(|s| s.predicted.map_or(f64::NAN, |p| p.0 as f64))
            .collect();
        let (lo, hi) = xs
            .iter()
            .chain(pxs.iter().filter(|v| v.is_finite()))
            .fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        if let Some(token) =
            plot.begin_plot_with_size("x (cells)", [width, crate::ui::widgets::px(ui, 200.0)])
        {
            plot.setup_axes_limits(
                -(self.seconds as f64),
                0.0,
                lo - 2.0,
                hi + 2.0,
                PlotCond::Always,
            );
            LinePlot::new("client", &t, &xs).plot(&plot);
            LinePlot::new("server prediction", &t, &pxs).plot(&plot);
            token.end();
        }
    }

    /// The worst frames of the window: time, client cell, predicted cell, delta.
    fn delta_table(&self, ui: &Ui, samples: &[crate::devlink::Sample], pred: &[f64], now: u64) {
        let mut rows: Vec<usize> = (0..samples.len())
            .filter(|&i| pred[i] > crate::devlink::DESYNC_CELLS as f64)
            .collect();
        rows.sort_by(|&a, &b| pred[b].total_cmp(&pred[a]));
        rows.truncate(30);
        let cell =
            |p: Option<(f32, f32)>| p.map_or("-".to_string(), |p| format!("{:.1},{:.1}", p.0, p.1));
        let tsv = |i: usize| {
            let s = &samples[i];
            format!(
                "{}\t{:.1},{:.1}\t{}\t{:.2}",
                s.unix_ms as i64 - now as i64,
                s.client.0,
                s.client.1,
                cell(s.predicted),
                pred[i]
            )
        };
        if ui.button("Copy TSV") {
            let mut text = String::from("ms\tclient cell\tserver cell\tdelta\n");
            for &i in &rows {
                text.push_str(&tsv(i));
                text.push('\n');
            }
            crate::ui::set_clipboard(&text);
        }
        if rows.is_empty() {
            widgets::empty(ui, "No frame is over the limit.");
            return;
        }
        widgets::counter(
            ui,
            &format!(
                "{} over the limit (worst 30 shown)",
                widgets::plural(rows.len(), "frame")
            ),
        );
        if let Some(_t) = ui.begin_table_header_with_flags(
            "desync_rows",
            ["t (ms)", "client cell", "server cell", "delta", "reason"]
                .map(imgui::TableColumnSetup::new),
            crate::ui::table_opts(),
        ) {
            for &i in &rows {
                let s = &samples[i];
                ui.table_next_column();
                ui.text((s.unix_ms as i64 - now as i64).to_string());
                ui.table_next_column();
                ui.text(format!("{:.1}, {:.1}", s.client.0, s.client.1));
                ui.table_next_column();
                ui.text(cell(s.predicted));
                ui.table_next_column();
                ui.text_colored(super::theme::WARN, format!("{:.2}", pred[i]));
                ui.table_next_column();
                ui.text_disabled("prediction");
            }
        }
    }
}
