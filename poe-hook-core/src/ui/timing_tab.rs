//! Timing tab (ImPlot): frame time, messages per 100 ms in each direction and the client's
//! lockstep backlog over the last seconds. The same series go into snapshots (`timings.csv`).

use dear_implot::{ImPlotExt, LinePlot, Plot, PlotCond, PlotContext, StemPlot};
use imgui::Ui;

use super::widgets;

use crate::timing::Series;

pub struct TimingView {
    seconds: i32,
    /// Samples before this time are ignored (the Reset button).
    reset_at: Option<u64>,
}

impl TimingView {
    pub fn new() -> Self {
        Self {
            seconds: 10,
            reset_at: None,
        }
    }

    pub fn draw(&mut self, ui: &Ui, plots: &PlotContext) {
        widgets::header(
            ui,
            "Frame time, messages per 100 ms and the lockstep backlog.",
            "The same series go into snapshots (timings.csv).",
        );
        ui.set_next_item_width(crate::ui::widgets::px(ui, 200.0));
        ui.slider("seconds", 2, 120, &mut self.seconds);
        ui.same_line();
        if ui.button("Reset") {
            self.reset_at = Some(poe_hook_shared::unix_ms());
        }
        let now = poe_hook_shared::unix_ms();
        let mut since = now.saturating_sub(self.seconds as u64 * 1000);
        if let Some(r) = self.reset_at {
            since = since.max(r);
        }
        let t = |ms: u64| (ms as f64 - now as f64) / 1000.0;
        let frame = crate::timing::samples(Series::FrameMs, since);
        let (fx, fy): (Vec<f64>, Vec<f64>) =
            frame.iter().map(|s| (t(s.unix_ms), s.value as f64)).unzip();
        let worst = fy.iter().cloned().fold(0.0, f64::max);
        let avg = if fy.is_empty() {
            0.0
        } else {
            fy.iter().sum::<f64>() / fy.len() as f64
        };
        let backlog_s = crate::timing::samples(Series::Backlog, since);
        summary(ui, &fy, &backlog_s);
        let _ = avg;
        let plot = ui.implot(plots);
        let width = ui.content_region_avail()[0];
        let x_min = -(self.seconds as f64);
        if let Some(token) = plot.begin_plot_with_size(
            "Frame time (ms)",
            [width, crate::ui::widgets::px(ui, 220.0)],
        ) {
            plot.setup_axes_limits(x_min, 0.0, 0.0, (worst * 1.2).max(10.0), PlotCond::Always);
            LinePlot::new("frame ms", &fx, &fy).plot(&plot);
            token.end();
        }
        // Messages per 100 ms bucket.
        let buckets = (self.seconds * 10) as usize;
        let count = |series: Series| {
            let mut v = vec![0.0f64; buckets];
            for s in crate::timing::samples(series, since) {
                let i = ((s.unix_ms - since) / 100) as usize;
                if i < buckets {
                    v[i] += 1.0;
                }
            }
            v
        };
        let xs: Vec<f64> = (0..buckets).map(|i| x_min + i as f64 * 0.1).collect();
        let (inn, out) = (count(Series::PacketIn), count(Series::PacketOut));
        let top = inn.iter().chain(&out).cloned().fold(1.0, f64::max);
        if let Some(token) = plot.begin_plot_with_size(
            "Messages per 100 ms",
            [width, crate::ui::widgets::px(ui, 200.0)],
        ) {
            plot.setup_axes_limits(x_min, 0.0, 0.0, top * 1.2, PlotCond::Always);
            StemPlot::new("received", &xs, &inn).plot(&plot);
            LinePlot::new("sent", &xs, &out).plot(&plot);
            token.end();
        }
        let backlog = crate::timing::samples(Series::Backlog, since);
        let (bx, by): (Vec<f64>, Vec<f64>) = backlog
            .iter()
            .map(|s| (t(s.unix_ms), s.value as f64))
            .unzip();
        let top = by.iter().cloned().fold(2.0, f64::max);
        if let Some(token) = plot.begin_plot_with_size(
            "Lockstep backlog (frames queued in the world)",
            [width, crate::ui::widgets::px(ui, 200.0)],
        ) {
            plot.setup_axes_limits(x_min, 0.0, 0.0, top * 1.2, PlotCond::Always);
            LinePlot::new("frames", &bx, &by).plot(&plot);
            token.end();
        }
    }
}

/// min / mean / p99 / max of a series.
fn stats(values: &[f64]) -> Option<(f64, f64, f64, f64)> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let p99 = v[((v.len() as f64 * 0.99).ceil() as usize).clamp(1, v.len()) - 1];
    Some((v[0], mean, p99, v[v.len() - 1]))
}

fn summary(ui: &Ui, frame_ms: &[f64], backlog: &[crate::timing::Sample]) {
    let backlog: Vec<f64> = backlog.iter().map(|s| s.value as f64).collect();
    if let Some(_t) = ui.begin_table_header_with_flags(
        "timing_summary",
        ["series", "min", "mean", "p99", "max"].map(imgui::TableColumnSetup::new),
        super::table_opts(),
    ) {
        for (name, values) in [("frame ms", frame_ms), ("backlog frames", &backlog[..])] {
            ui.table_next_column();
            ui.text(name);
            match stats(values) {
                Some((min, mean, p99, max)) => {
                    for v in [min, mean, p99, max] {
                        ui.table_next_column();
                        ui.text(format!("{v:.2}"));
                    }
                }
                None => {
                    for _ in 0..4 {
                        ui.table_next_column();
                        ui.text_disabled("-");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::stats;

    #[test]
    fn summary_stats_of_a_series() {
        assert_eq!(stats(&[]), None);
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        let (min, mean, p99, max) = stats(&v).unwrap();
        assert_eq!((min, max, p99), (1.0, 100.0, 99.0));
        assert!((mean - 50.5).abs() < 1e-9);
    }
}
