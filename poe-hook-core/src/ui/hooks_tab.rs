//! Hooks tab: time spent in each of our hook callbacks (they run on game threads, so their
//! time is the game's time). A callback over 50 ms is also logged and shown as a notice.

use std::time::Instant;

use imgui::Ui;

use super::widgets;

pub struct HooksView {
    /// Previous totals, for per-second rates.
    last: Vec<(&'static str, u64, u64)>,
    last_at: Instant,
    rates: Vec<(f64, f64)>,
}

impl HooksView {
    pub fn new() -> Self {
        Self {
            last: Vec::new(),
            last_at: Instant::now(),
            rates: Vec::new(),
        }
    }

    pub fn draw(&mut self, ui: &Ui) {
        let stats = crate::hooks::stats();
        // Rates over the last second.
        if self.last_at.elapsed().as_secs_f64() >= 1.0 || self.rates.len() != stats.len() {
            let dt = self.last_at.elapsed().as_secs_f64().max(0.001);
            self.rates = stats
                .iter()
                .map(|(n, calls, total, _)| {
                    let prev = self.last.iter().find(|(m, _, _)| m == n);
                    let (pc, pt) = prev.map_or((*calls, *total), |(_, c, t)| (*c, *t));
                    ((calls - pc) as f64 / dt, (total - pt) as f64 / 1e6 / dt)
                })
                .collect();
            self.last = stats.iter().map(|(n, c, t, _)| (*n, *c, *t)).collect();
            self.last_at = Instant::now();
        }
        widgets::header(
            ui,
            "Time spent in each of our hook callbacks, sorted by ms/s.",
            "They run on game threads, so their time is the game's time. ms/s = milliseconds of \
             game-thread time per second. A callback over 50 ms is also logged and shown as a \
             notice (Settings can turn the popup off).",
        );
        if ui.button("Reset counters") {
            crate::hooks::reset_stats();
            self.last.clear();
        }
        widgets::counter(ui, &widgets::plural(stats.len(), "callback"));
        if stats.is_empty() {
            widgets::empty(ui, "No hooks installed.");
            return;
        }
        let mut order: Vec<usize> = (0..stats.len()).collect();
        order.sort_by(|&a, &b| {
            let load = |i: usize| self.rates.get(i).map_or(0.0, |r| r.1);
            load(b).total_cmp(&load(a))
        });
        if let Some(_t) = crate::ui::list_table(
            ui,
            "hook_stats",
            [
                imgui::TableColumnSetup::new("callback"),
                imgui::TableColumnSetup::new("calls"),
                imgui::TableColumnSetup::new("calls/s"),
                imgui::TableColumnSetup::new("avg (us)"),
                imgui::TableColumnSetup::new("max ms"),
                imgui::TableColumnSetup::new("ms/s"),
            ],
        ) {
            for i in order {
                let (name, calls, total, max) = &stats[i];
                let (rate, load) = self.rates.get(i).copied().unwrap_or_default();
                ui.table_next_column();
                ui.text(name);
                ui.table_next_column();
                ui.text(calls.to_string());
                ui.table_next_column();
                ui.text(format!("{rate:.0}"));
                ui.table_next_column();
                ui.text(format!(
                    "{:.1}",
                    if *calls > 0 {
                        *total as f64 / *calls as f64 / 1000.0
                    } else {
                        0.0
                    }
                ));
                ui.table_next_column();
                let max_ms = *max as f64 / 1e6;
                if max_ms > 16.0 {
                    ui.text_colored(super::theme::BAD, format!("{max_ms:.2}"));
                } else {
                    ui.text(format!("{max_ms:.2}"));
                }
                ui.table_next_column();
                if load > 2.0 {
                    ui.text_colored(super::theme::WARN, format!("{load:.2}"));
                } else {
                    ui.text(format!("{load:.2}"));
                }
            }
        }
    }
}
