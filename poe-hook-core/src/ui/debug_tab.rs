//! Debug page (our server only): the server's commands as forms and a console. Tabs: Help
//! (every command, searchable, with its form), Console (raw command line), and one tab per
//! feature area. The area tabs are shells until their step lands; World works today.

use imgui::Ui;

use super::{
    Page,
    commands_tab::CommandsView,
    instances_tab::InstancesView,
    widgets::{self, Tone},
};
use crate::devlink::{self, Cmd};

pub struct DebugView {
    forms: CommandsView,
    instances: InstancesView,
    help_filter: String,
    help_sel: Option<String>,
    console: String,
    history: Vec<String>,
    hist_pos: Option<usize>,
    /// Changes the console input's id so it reads the buffer again after a recall.
    nonce: u32,
    refocus: bool,
    last_request: Option<std::time::Instant>,
    tx: i32,
    ty: i32,
}

impl DebugView {
    pub fn new() -> Self {
        Self {
            forms: CommandsView::new(),
            instances: InstancesView::new(),
            help_filter: String::new(),
            help_sel: None,
            console: String::new(),
            history: Vec::new(),
            hist_pos: None,
            nonce: 0,
            refocus: false,
            last_request: None,
            tx: 0,
            ty: 0,
        }
    }

    pub fn draw(&mut self, ui: &Ui, page: Page) {
        match page {
            Page::DebugHelp => self.help(ui),
            Page::DebugConsole => self.console(ui),
            Page::DebugWorld => self.world(ui),
            Page::DebugCharacter => shell(
                ui,
                "Level, XP, points and class",
                "P9 (levels and XP), P15 (passives)",
            ),
            Page::DebugStats => shell(ui, "Stat editor", "P9 (items, stats pipeline)"),
            Page::DebugItems => shell(ui, "Item creator", "P11 (item creator)"),
            Page::DebugMonsters => shell(ui, "Monster spawner and inspector", "P17 (monsters)"),
            Page::DebugLife => shell(
                ui,
                "Life, mana and energy shield",
                "P13 (life, damage, death)",
            ),
            Page::DebugPlayers => shell(
                ui,
                "Online players, kick, mute, announce",
                "P7 (command framework)",
            ),
            _ => {}
        }
        ui.separator();
        results_strip(ui);
    }

    /// The registry, asking the server again now and then while it is empty.
    fn registry(&mut self, ui: &Ui) -> Option<(Vec<Cmd>, Option<devlink::Me>)> {
        let (cmds, me) = devlink::registry();
        if cmds.is_empty() {
            widgets::empty(
                ui,
                "No command list from the server yet (needs position sync and [instance] debug_link).",
            );
            if self.last_request.is_none_or(|t| t.elapsed().as_secs() >= 3)
                && devlink::state().is_some()
            {
                devlink::command("registry");
                self.last_request = Some(std::time::Instant::now());
            }
            return None;
        }
        Some((cmds, me))
    }

    fn help(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "Every server command with its usage; pick one to get its form.",
            "The same commands work in chat as !name. Aliases and flags come with the command \
             framework (P7).",
        );
        let Some((cmds, me)) = self.registry(ui) else {
            return;
        };
        let admin = me.as_ref().is_some_and(|m| m.admin);
        let prefix = me.as_ref().map_or("!".to_string(), |m| m.prefix.clone());
        let cmds: Vec<Cmd> = cmds.into_iter().filter(|c| admin || !c.admin).collect();
        if let Some(m) = &me {
            widgets::chip(
                ui,
                if admin { "ADMIN" } else { "player" },
                if admin { Tone::Warn } else { Tone::Muted },
            );
            ui.same_line();
            ui.text_disabled(&m.account);
            ui.same_line();
        }
        let mut f = std::mem::take(&mut self.help_filter);
        widgets::filter(ui, "help_filter", &mut f, "search commands", 240.0);
        self.help_filter = f;
        let q = self.help_filter.to_lowercase();
        let shown: Vec<&Cmd> = cmds
            .iter()
            .filter(|c| {
                q.is_empty()
                    || c.name.to_lowercase().contains(&q)
                    || c.help.to_lowercase().contains(&q)
                    || c.group.to_lowercase().contains(&q)
            })
            .collect();
        widgets::counter(ui, &format!("{} commands", shown.len()));
        let mut groups: Vec<&str> = Vec::new();
        for c in &shown {
            if !groups.contains(&c.group.as_str()) {
                groups.push(&c.group);
            }
        }
        let height = (ui.content_region_avail()[1] - 150.0).max(160.0);
        let _ = height;
        let sel = self.help_sel.clone();
        let mut picked = None;
        let forms = &mut self.forms;
        let avail_h = ui.content_region_avail()[1] - 170.0;
        ui.child_window("help_box")
            .size([0.0, avail_h.max(160.0)])
            .build(ui, || {
                widgets::split(
                    ui,
                    "debug_help",
                    240.0,
                    || {
                        for g in &groups {
                            ui.text_disabled(g);
                            for c in shown.iter().filter(|c| c.group == *g) {
                                if ui
                                    .selectable_config(format!("  {}##h{}", c.name, c.name))
                                    .selected(sel.as_deref() == Some(c.name.as_str()))
                                    .build()
                                {
                                    picked = Some(c.name.clone());
                                }
                            }
                        }
                    },
                    || match shown
                        .iter()
                        .find(|c| sel.as_deref() == Some(c.name.as_str()))
                    {
                        Some(c) => {
                            ui.text(usage(c, &prefix));
                            ui.text_wrapped(&c.help);
                            ui.text_disabled(format!(
                                "Permission: {}   Group: {}",
                                if c.admin { "admin" } else { "everyone" },
                                c.group
                            ));
                            ui.separator();
                            forms.draw_command(ui, c, admin, &prefix);
                        }
                        None => widgets::empty(ui, "Pick a command on the left."),
                    },
                );
            });
        if picked.is_some() {
            self.help_sel = picked;
        }
    }

    fn console(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "Run any server command; results come back as ack lines.",
            "Up/Down recall earlier lines, Tab completes the command name.",
        );
        let Some((cmds, me)) = self.registry(ui) else {
            return;
        };
        let admin = me.as_ref().is_some_and(|m| m.admin);
        let prefix = me.as_ref().map_or("!".to_string(), |m| m.prefix.clone());
        let h = (ui.content_region_avail()[1] - 120.0).max(100.0);
        ui.child_window("console_out")
            .size([0.0, h])
            .border(true)
            .build(ui, || {
                let results = devlink::results();
                if results.is_empty() {
                    widgets::empty(ui, "Nothing run yet.");
                }
                for (ms, cmd, result) in results.iter() {
                    ui.text_disabled(format!("{} > {prefix}{cmd}", crate::util::clock(*ms)));
                    let color = if result.starts_with("ok") {
                        super::theme::OK
                    } else {
                        super::theme::BAD
                    };
                    ui.text_colored(color, format!("  {result}"));
                }
                if ui.scroll_y() >= ui.scroll_max_y() - 40.0 {
                    ui.set_scroll_here_y(1.0);
                }
            });
        let names: Vec<&str> = cmds
            .iter()
            .filter(|c| admin || !c.admin)
            .map(|c| c.name.as_str())
            .collect();
        let word = self
            .console
            .trim_start_matches(&*prefix)
            .split(' ')
            .next()
            .unwrap_or("")
            .to_string();
        let one_word = !self.console.trim_start().contains(' ');
        let matches: Vec<&str> = names
            .iter()
            .copied()
            .filter(|n| n.starts_with(&word))
            .collect();
        if self.refocus {
            self.refocus = false;
            ui.set_keyboard_focus_here();
        }
        let _id = ui.push_id(self.nonce as usize);
        ui.set_next_item_width(-70.0);
        let enter = ui
            .input_text("##console", &mut self.console)
            .hint("command, e.g. help")
            .enter_returns_true(true)
            .build();
        let active = ui.is_item_active() || enter;
        ui.same_line();
        let run = ui.button("Run");
        if active && ui.is_key_pressed(imgui::Key::UpArrow) && !self.history.is_empty() {
            let p = self
                .hist_pos
                .map_or(self.history.len() - 1, |p| p.saturating_sub(1));
            self.hist_pos = Some(p);
            self.console = self.history[p].clone();
            self.nonce += 1;
            self.refocus = true;
        }
        if active
            && ui.is_key_pressed(imgui::Key::DownArrow)
            && let Some(p) = self.hist_pos
        {
            if p + 1 < self.history.len() {
                self.hist_pos = Some(p + 1);
                self.console = self.history[p + 1].clone();
            } else {
                self.hist_pos = None;
                self.console.clear();
            }
            self.nonce += 1;
            self.refocus = true;
        }
        if active && one_word && ui.is_key_pressed(imgui::Key::Tab) && !matches.is_empty() {
            self.console = common_prefix(&matches);
            if matches.len() == 1 {
                self.console.push(' ');
            }
            self.nonce += 1;
            self.refocus = true;
        }
        if enter || run {
            let line = self
                .console
                .trim()
                .trim_start_matches(&*prefix)
                .trim()
                .to_string();
            if !line.is_empty() {
                devlink::command(&line);
                if self.history.last() != Some(&line) {
                    self.history.push(line);
                }
                self.hist_pos = None;
                self.console.clear();
                self.nonce += 1;
                self.refocus = true;
            }
        }
        if !word.is_empty() && one_word {
            let list: Vec<&str> = matches.iter().copied().take(8).collect();
            ui.text_disabled(format!("suggestions: {}", list.join(" | ")));
        } else {
            ui.text_disabled(" ");
        }
    }

    fn world(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "Instances on the server, a new-instance form and teleport.",
            "Chat equivalents: !transfer <area> [seed] [topology] [orientation], !instance_new, \
             !join <id>, !close <id>, !teleport <x> <y>.",
        );
        ui.text("Teleport");
        ui.same_line();
        ui.set_next_item_width(100.0);
        ui.input_int("x##tp", &mut self.tx);
        ui.same_line();
        ui.set_next_item_width(100.0);
        ui.input_int("y##tp", &mut self.ty);
        ui.same_line();
        if ui.button("Teleport") {
            devlink::command(&format!("teleport {} {}", self.tx, self.ty));
        }
        ui.same_line();
        if ui.button("Here")
            && let Some(p) = crate::inspect::player()
        {
            self.tx = p.grid.0;
            self.ty = p.grid.1;
        }
        ui.separator();
        let h = (ui.content_region_avail()[1] - 120.0).max(160.0);
        ui.child_window("world_box").size([0.0, h]).build(ui, || {
            self.instances.draw(ui);
        });
    }
}

fn shell(ui: &Ui, what: &str, step: &str) {
    widgets::header(ui, what, "");
    ui.spacing();
    ui.text_disabled(format!("Comes with {step}."));
    ui.text_disabled("Until then the Help and Console tabs run the commands that exist.");
}

/// The usage line: `!name <required> [optional]`.
fn usage(c: &Cmd, prefix: &str) -> String {
    let args: Vec<String> = c
        .args
        .iter()
        .map(|a| {
            if a.optional {
                format!("[{}]", a.name)
            } else {
                format!("<{}>", a.name)
            }
        })
        .collect();
    format!("{prefix}{} {}", c.name, args.join(" "))
        .trim_end()
        .to_string()
}

fn common_prefix(items: &[&str]) -> String {
    let mut p = items[0].to_string();
    for s in &items[1..] {
        while !s.starts_with(&p) {
            p.pop();
        }
    }
    p
}

/// The last 20 ack lines of any command, at the bottom of every Debug tab.
fn results_strip(ui: &Ui) {
    ui.text_disabled("Results");
    ui.child_window("results").border(true).build(ui, || {
        let all = devlink::results();
        if all.is_empty() {
            widgets::empty(ui, "No command run yet.");
        }
        for (ms, cmd, result) in all.iter().rev().take(20) {
            let color = if result.starts_with("ok") {
                super::theme::OK
            } else {
                super::theme::BAD
            };
            ui.text_colored(
                color,
                format!("{} {cmd} -> {result}", crate::util::clock(*ms)),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::common_prefix;

    #[test]
    fn completion_stops_where_the_names_differ() {
        assert_eq!(common_prefix(&["item_give", "item_edit"]), "item_");
        assert_eq!(common_prefix(&["teleport"]), "teleport");
    }
}
