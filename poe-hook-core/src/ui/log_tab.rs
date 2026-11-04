//! Log page: the hook's log records (loader and core) and this launch's lines of the game's own
//! `Client.txt`, in one viewer. The hook records are also in `poe-hook.log` (next to the exe
//! and in the session folder).

use imgui::{ListClipper, Ui};

use super::{
    follow::Follow,
    widgets::{self, Tone},
};

const KEEP: usize = 50_000;

struct Rec {
    level: u32,
    line: String,
    /// `HH:MM:SS`, to merge the two sources.
    key: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Hook,
    Client,
    Both,
}

/// Levels `[all, info, warn, error]`: the lowest level number (1 = error … 5 = trace) shown.
const LEVELS: [&str; 4] = ["all", "info", "warn", "error"];
const MAX_LEVEL: [u32; 4] = [5, 3, 2, 1];

pub struct LogView {
    records: Vec<Rec>,
    client: Vec<Rec>,
    next: u64,
    filter: String,
    source: Source,
    level: usize,
    follow: Follow,
    /// (is client, index) of the rows that pass the filter, in display order.
    shown: Vec<(bool, usize)>,
    shown_for: (String, usize, usize, usize, u8, usize),
}

fn client_level(l: &str) -> u32 {
    if l.contains("[CRIT") || l.contains("[ERROR") {
        1
    } else if l.contains("[WARN") {
        2
    } else {
        3
    }
}

impl LogView {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            client: Vec::new(),
            next: 0,
            filter: String::new(),
            source: Source::Hook,
            level: 0,
            follow: Follow::new(),
            shown: Vec::new(),
            shown_for: (String::new(), 0, 0, 0, 0, 0),
        }
    }

    /// Called every drawn frame so no record is missed while the page is hidden.
    pub fn pull(&mut self) {
        let records = &mut self.records;
        self.next = crate::host::logs_since(self.next, |r| {
            records.push(Rec {
                level: r.level,
                line: format!(
                    "{} {:5} [{}] {}",
                    crate::util::clock(r.unix_ms),
                    crate::util::level_name(r.level),
                    unsafe { r.target.as_str() },
                    unsafe { r.message.as_str() }
                ),
                key: crate::util::clock(r.unix_ms),
            });
        });
        if self.records.len() > KEEP {
            let drop = self.records.len() - KEEP;
            self.records.drain(..drop);
            self.shown_for.5 = usize::MAX;
        }
    }

    fn pull_client(&mut self) {
        let lines = crate::gamelog::lines();
        let same = lines.len() == self.client.len()
            && lines.last().map(String::as_str) == self.client.last().map(|r| r.line.as_str());
        if same {
            return;
        }
        self.client = lines
            .into_iter()
            .map(|l| Rec {
                level: client_level(&l),
                key: l.get(11..19).unwrap_or("").to_string(),
                line: l,
            })
            .collect();
    }

    fn passes(&self, r: &Rec, f: &str) -> bool {
        r.level <= MAX_LEVEL[self.level] && (f.is_empty() || r.line.to_lowercase().contains(f))
    }

    fn refresh(&mut self) {
        let want_hook = self.source != Source::Client;
        let want_client = self.source != Source::Hook;
        let key = (
            self.filter.clone(),
            self.level,
            self.source as usize,
            if want_hook { self.records.len() } else { 0 },
            0u8,
            if want_client { self.client.len() } else { 0 },
        );
        if self.shown_for == key {
            return;
        }
        self.shown_for = key;
        let f = self.filter.to_lowercase();
        let hook: Vec<usize> = (0..self.records.len())
            .filter(|&i| want_hook && self.passes(&self.records[i], &f))
            .collect();
        let client: Vec<usize> = (0..self.client.len())
            .filter(|&i| want_client && self.passes(&self.client[i], &f))
            .collect();
        // Merge by time of day (both lists are in time order).
        let (mut a, mut b) = (0, 0);
        self.shown.clear();
        while a < hook.len() || b < client.len() {
            let take_hook = b >= client.len()
                || (a < hook.len() && self.records[hook[a]].key <= self.client[client[b]].key);
            if take_hook {
                self.shown.push((false, hook[a]));
                a += 1;
            } else {
                self.shown.push((true, client[b]));
                b += 1;
            }
        }
    }

    pub fn draw(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "Hook log and the game's Client.txt (this launch).",
            "Hook records are also in poe-hook.log next to the exe and in the session folder. \
             Client.txt rows are tagged CLIENT.",
        );
        if self.source != Source::Hook {
            self.pull_client();
        }
        widgets::filter(ui, "log_filter", &mut self.filter, "filter", 260.0);
        ui.same_line();
        let mut s = self.source as usize;
        ui.set_next_item_width(80.0);
        if ui.combo_simple_string("##source", &mut s, &["hook", "client", "both"]) {
            self.source = [Source::Hook, Source::Client, Source::Both][s];
        }
        ui.same_line();
        ui.set_next_item_width(80.0);
        ui.combo_simple_string("##level", &mut self.level, &LEVELS);
        ui.same_line();
        let mut follow = self.follow.following();
        if ui.checkbox("Follow", &mut follow) {
            if follow {
                self.follow.jump();
            } else {
                self.follow.pause(self.shown.len());
            }
        }
        ui.same_line();
        self.refresh();
        if ui.button("Copy shown") {
            let text: Vec<&str> = self
                .shown
                .iter()
                .map(|&(c, i)| {
                    if c { &self.client[i] } else { &self.records[i] }
                        .line
                        .as_str()
                })
                .collect();
            crate::ui::set_clipboard(&text.join("\n"));
        }
        ui.same_line();
        if ui.button("Open folder") {
            // Best effort (under Wine `explorer` may do nothing).
            let _ = std::process::Command::new("explorer")
                .arg(crate::host::session_dir())
                .spawn();
        }
        widgets::counter(
            ui,
            &format!(
                "{} of {} rows",
                widgets::thousands(self.shown.len()),
                widgets::thousands(self.records.len() + self.client.len())
            ),
        );
        if self.shown.is_empty() {
            widgets::empty(ui, "No log rows match.");
        }
        ui.child_window("log_lines")
            .flags(imgui::WindowFlags::HORIZONTAL_SCROLLBAR)
            .build(ui, || {
                self.follow.begin(ui, self.shown.len());
                let mut clipper = ListClipper::new(self.shown.len()).begin(ui);
                while clipper.step() {
                    for row in clipper.display_start()..clipper.display_end() {
                        let (c, i) = self.shown[row];
                        let r = if c { &self.client[i] } else { &self.records[i] };
                        if c {
                            widgets::chip(ui, "CLIENT", Tone::Muted);
                            ui.same_line();
                        }
                        match r.level {
                            1 => ui.text_colored(super::theme::BAD, &r.line),
                            2 => ui.text_colored(super::theme::WARN, &r.line),
                            4 | 5 => ui.text_colored(super::theme::muted(ui), &r.line),
                            _ => ui.text(&r.line),
                        }
                    }
                }
                self.follow.end(ui, self.shown.len());
            });
    }
}
