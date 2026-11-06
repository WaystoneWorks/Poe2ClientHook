//! Stats and Actions tabs: the player's stats by name (live, changed values highlighted) and
//! the trace of action starts and ends.

use std::{collections::HashMap, time::Instant};

use imgui::{ListClipper, Ui};

use super::widgets;

pub struct StatsView {
    filter: String,
    changed_only: bool,
    set: usize,
    /// (set, key) → (value, when it last changed; `None` = not since it was first seen).
    seen: HashMap<(u32, u32), (i32, Option<Instant>)>,
}

impl StatsView {
    pub fn new() -> Self {
        Self {
            filter: String::new(),
            changed_only: false,
            set: 0,
            seen: HashMap::new(),
        }
    }

    pub fn draw(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "The player's stats by name, live; yellow = changed in the last 3 s.",
            "Set 0 is the player's own stats; names come from Stats.dat. A server column for \
             comparing with the server's values comes with the stats pipeline (P10).",
        );
        let sets = crate::inspect::player_stats();
        widgets::filter(
            ui,
            "stats_filter",
            &mut self.filter,
            "filter stat id/name",
            260.0,
        );
        ui.same_line();
        ui.checkbox("Changed only", &mut self.changed_only);
        ui.same_line();
        let mut items = vec!["all sets".to_string()];
        items.extend(sets.iter().map(|(s, _)| {
            if *s == 0 {
                "0 own".to_string()
            } else {
                s.to_string()
            }
        }));
        self.set = self.set.min(items.len() - 1);
        ui.set_next_item_width(crate::ui::widgets::px(ui, 100.0));
        ui.combo_simple_string("##stat_set", &mut self.set, &items);
        if sets.is_empty() {
            widgets::empty(ui, "No player stats yet: enter an area.");
            return;
        }
        let want_set = (self.set > 0).then(|| sets[self.set - 1].0);
        let f = self.filter.to_lowercase();
        let now = Instant::now();
        let mut rows = Vec::new();
        let mut in_set = 0;
        for (set, pairs) in &sets {
            for &(key, value) in pairs {
                let name = crate::gamedata::stat_name(key).unwrap_or("?");
                let changed = match self.seen.get(&(*set, key)) {
                    Some((v, t)) if *v == value => *t,
                    Some(_) => Some(now),
                    None => None,
                };
                self.seen.insert((*set, key), (value, changed));
                let recent = changed.is_some_and(|t| now.duration_since(t).as_secs_f32() < 3.0);
                if want_set.is_some_and(|w| w != *set) {
                    continue;
                }
                in_set += 1;
                if (!self.changed_only || recent)
                    && (f.is_empty() || name.contains(&f) || key.to_string() == f)
                {
                    rows.push((*set, key, name, value, recent));
                }
            }
        }
        widgets::counter(
            ui,
            &format!(
                "{} of {}",
                widgets::thousands(rows.len()),
                widgets::plural(in_set, "stat")
            ),
        );
        if rows.is_empty() {
            widgets::empty(
                ui,
                if self.changed_only && f.is_empty() {
                    "Nothing changed in the last 3 s."
                } else {
                    "No stat matches the filter."
                },
            );
            return;
        }
        if let Some(_t) = crate::ui::list_table(
            ui,
            "stats",
            [
                imgui::TableColumnSetup::new("set"),
                imgui::TableColumnSetup::new("key"),
                imgui::TableColumnSetup::new("stat"),
                imgui::TableColumnSetup::new("value"),
            ],
            "stat",
        ) {
            let mut clipper = ListClipper::new(rows.len()).begin(ui);
            while clipper.step() {
                for i in clipper.display_start()..clipper.display_end() {
                    let (set, key, name, value, recent) = rows[i];
                    ui.table_next_column();
                    ui.text(set.to_string());
                    ui.table_next_column();
                    ui.text(key.to_string());
                    ui.table_next_column();
                    ui.text(name);
                    ui.table_next_column();
                    if recent {
                        ui.text_colored(super::theme::WARN, value.to_string());
                    } else {
                        ui.text(value.to_string());
                    }
                }
            }
        }
    }
}

pub struct ActionsView {
    paused: Option<Vec<crate::actions::Event>>,
}

impl ActionsView {
    pub fn new() -> Self {
        Self { paused: None }
    }

    pub fn draw(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "The player's Actor actions (slot 0 = +0x238, 1 = +0x240), newest first.",
            "Names come from ActionTypes.dat.",
        );
        let mut paused = self.paused.is_some();
        if ui.checkbox("Freeze", &mut paused) {
            self.paused = paused.then(crate::actions::events);
        }
        if ui.is_item_hovered() {
            ui.tooltip_text("Hold this list still (actions are still recorded).");
        }
        ui.same_line();
        if ui.button("Clear") {
            crate::actions::clear();
            self.paused = None;
        }
        let events = self.paused.clone().unwrap_or_else(crate::actions::events);
        widgets::counter(ui, &widgets::plural(events.len(), "event"));
        if events.is_empty() {
            widgets::empty(ui, "No actions yet: move or use a skill.");
            return;
        }
        if let Some(_t) = crate::ui::list_table(
            ui,
            "actions",
            ["time", "frame", "slot", "", "action", "type id", "id"]
                .map(imgui::TableColumnSetup::new),
            "action",
        ) {
            let mut clipper = ListClipper::new(events.len()).begin(ui);
            while clipper.step() {
                for i in clipper.display_start()..clipper.display_end() {
                    let e = &events[events.len() - 1 - i];
                    ui.table_next_column();
                    ui.text(crate::util::clock(e.unix_ms));
                    ui.table_next_column();
                    ui.text(e.frame.to_string());
                    ui.table_next_column();
                    ui.text(e.slot.to_string());
                    ui.table_next_column();
                    if e.started {
                        ui.text_colored(super::theme::OK, "start");
                    } else {
                        ui.text_disabled("end");
                    }
                    ui.table_next_column();
                    ui.text(e.name());
                    ui.table_next_column();
                    ui.text(e.kind.to_string());
                    ui.table_next_column();
                    ui.text(format!("{:#x}", e.action));
                }
            }
        }
    }
}
