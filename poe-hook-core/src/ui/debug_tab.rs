//! Debug page (our server only): the server's commands. Tabs: Help (every command, searchable,
//! with its form), Console (raw command line with completion), and one tab per feature area:
//! hand-laid controls for the common actions, then a generated form for every other command the
//! registry assigns to the tab, so nothing lacks an overlay counterpart.

use imgui::Ui;
use poe_hook_shared::cmdspec::{Suggest, common_prefix, quote, suggest};

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
    /// The name list request last sent: asked once per word typed.
    asked: Option<(String, String)>,
    tx: i32,
    ty: i32,
    scale: String,
    /// Act number (WorldAreas.Act; the interludes are 6) and area id for the travel unlocks.
    unlock_act: i32,
    unlock_area: String,
    announce: String,
    mute_for: String,
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
            asked: None,
            tx: 0,
            ty: 0,
            scale: "1".into(),
            unlock_act: 1,
            unlock_area: String::new(),
            announce: String::new(),
            mute_for: "30m".into(),
        }
    }

    pub fn draw(&mut self, ui: &Ui, page: Page) {
        match page {
            Page::DebugHelp => self.help(ui),
            Page::DebugConsole => self.console(ui),
            Page::DebugWorld => self.world(ui),
            Page::DebugPlayers => self.players(ui),
            Page::DebugCharacter => self.character(ui),
            Page::DebugStats => self.area_tab(
                ui,
                "Debug>Stats",
                "Stat editor",
                "P10 (items, stats pipeline)",
            ),
            Page::DebugItems => {
                self.area_tab(ui, "Debug>Items", "Item creator", "P12 (item creator)")
            }
            Page::DebugMonsters => self.area_tab(
                ui,
                "Debug>Monsters",
                "Monster spawner and inspector",
                "P17 (monsters)",
            ),
            Page::DebugLife => self.area_tab(
                ui,
                "Debug>Life",
                "Life, mana and energy shield",
                "P13 (life, damage, death)",
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
            "Every server command you may use; pick one for its usage and form.",
            "The same commands work in chat with the prefix. Search covers names, aliases and \
             summaries.",
        );
        let Some((cmds, me)) = self.registry(ui) else {
            return;
        };
        let admin = me.as_ref().is_some_and(|m| m.admin);
        let prefix = me.as_ref().map_or("!".to_string(), |m| m.prefix.clone());
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
                    || c.name().to_lowercase().contains(&q)
                    || c.summary.to_lowercase().contains(&q)
                    || c.aliases.iter().any(|a| a.to_lowercase().contains(&q))
            })
            .collect();
        widgets::counter(ui, &format!("{} commands", shown.len()));
        let mut groups: Vec<&str> = Vec::new();
        for c in &shown {
            if !groups.contains(&c.category.as_str()) {
                groups.push(&c.category);
            }
        }
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
                            for c in shown.iter().filter(|c| c.category == *g) {
                                let name = c.name();
                                if ui
                                    .selectable_config(format!("  {name}##h{name}"))
                                    .selected(sel.as_deref() == Some(name.as_str()))
                                    .build()
                                {
                                    picked = Some(name);
                                }
                            }
                        }
                    },
                    || match shown
                        .iter()
                        .find(|c| sel.as_deref() == Some(c.name().as_str()))
                    {
                        Some(c) => help_pane(ui, c, &prefix, forms),
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
            "Up/Down recall earlier lines. Tab completes command names, subcommands, flags, \
             choices and names from the game data (the list comes from the server).",
        );
        let Some((cmds, me)) = self.registry(ui) else {
            return;
        };
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
        // What can follow what is typed: from the registry, or asked of the server.
        let line = self.console.trim_start_matches(&*prefix).to_string();
        let typed = suggest(&cmds, &line);
        let (start, word) = (typed.start, typed.word.to_string());
        let offered: Vec<(String, String)> = match &typed.suggest {
            Suggest::Nothing => Vec::new(),
            Suggest::Words(words) => words.iter().map(|w| (w.clone(), String::new())).collect(),
            Suggest::Remote { kind, typed } => {
                let want = (kind.clone(), typed.clone());
                if self.asked.as_ref() != Some(&want) {
                    devlink::request_completion(kind, typed);
                    self.asked = Some(want);
                }
                devlink::completion()
                    .filter(|c| c.kind == *kind && c.typed == *typed)
                    .map_or(Vec::new(), |c| c.names)
            }
        };
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
        if active && ui.is_key_pressed(imgui::Key::Tab) && !offered.is_empty() {
            let ids: Vec<&str> = offered.iter().map(|o| o.0.as_str()).collect();
            let shared = common_prefix(&ids);
            let bare = word.trim_start_matches('"').to_lowercase();
            // Several matches found by a display name need not share a useful start.
            let insert = if ids.len() == 1 {
                Some(format!("{} ", quote(ids[0])))
            } else if shared.to_lowercase().starts_with(&bare) && shared.len() > bare.len() {
                Some(shared)
            } else {
                None
            };
            if let Some(insert) = insert {
                self.console = format!("{}{insert}", &line[..start]);
                self.nonce += 1;
                self.refocus = true;
            }
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
                self.asked = None;
                self.nonce += 1;
                self.refocus = true;
            }
        }
        // The suggestion strip: a click takes the word.
        ui.text_disabled("suggestions:");
        let mut take = None;
        for (i, (id, display)) in offered.iter().take(8).enumerate() {
            ui.same_line();
            let label = if display.is_empty() {
                format!("{id}##s{i}")
            } else {
                format!("{display} ({id})##s{i}")
            };
            if ui.small_button(label) {
                take = Some(id.clone());
            }
        }
        if let Some(id) = take {
            self.console = format!("{}{} ", &line[..start], quote(&id));
            self.nonce += 1;
            self.refocus = true;
        }
    }

    fn world(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "Instances on the server, a new-instance form, teleport, time and the area export.",
            "Chat equivalents: !go <area> [seed] [topology] [orientation], !instance new, \
             !join <id>, !instance close <id>, !tp <x> <y>, !stuck, !time scale <factor>.",
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
            devlink::command(&format!("tp {} {}", self.tx, self.ty));
        }
        ui.same_line();
        if ui.button("Here")
            && let Some(p) = crate::inspect::player()
        {
            self.tx = p.grid.0;
            self.ty = p.grid.1;
        }
        ui.same_line();
        if ui.button("Stuck") {
            devlink::command("stuck");
        }
        ui.text("Time scale");
        ui.same_line();
        ui.set_next_item_width(80.0);
        ui.input_text("##scale", &mut self.scale).build();
        ui.same_line();
        if ui.button("Apply##scale") {
            devlink::command(&format!("time scale {}", self.scale.trim()));
        }
        ui.same_line();
        if ui.button("Reset instance") {
            devlink::command("instance reset");
        }
        ui.same_line();
        if ui.button("Export area") {
            let ok = crate::dump_types::export_layout_now();
            crate::ui::notice(match crate::terrain_tools::export_now() {
                Some(dir) => format!(
                    "Area exported to {} (with server entities and walkability probe)",
                    dir.display()
                ),
                None if ok => {
                    "Area layout and grids exported (dump_dir terrain/layouts and terrain/grids)"
                        .into()
                }
                None => "No area loaded".to_string(),
            });
        }
        ui.same_line();
        ui.text_disabled(format!(
            "now {}   seed {}",
            devlink::live("time_scale").unwrap_or_else(|| "-".into()),
            devlink::live("seed").unwrap_or_else(|| "-".into())
        ));
        ui.text("Travel");
        ui.same_line();
        if ui.button("Unlock all acts") {
            devlink::command("waypoint unlock all");
        }
        ui.same_line();
        ui.set_next_item_width(90.0);
        ui.input_int("act (6 = interludes)##unlock", &mut self.unlock_act);
        ui.same_line();
        if ui.button("Unlock act") {
            devlink::command(&format!("waypoint unlock act {}", self.unlock_act));
        }
        ui.same_line();
        ui.set_next_item_width(110.0);
        ui.input_text("area##unlock", &mut self.unlock_area).build();
        ui.same_line();
        if ui.button("Unlock waypoint") && !self.unlock_area.trim().is_empty() {
            devlink::command(&format!(
                "waypoint unlock waypoint {}",
                self.unlock_area.trim()
            ));
        }
        ui.same_line();
        if ui.button("Unlock checkpoints here") {
            devlink::command("checkpoint unlock all");
        }
        ui.separator();
        let h = (ui.content_region_avail()[1] - 120.0).max(160.0);
        let (cmds, me) = devlink::registry();
        let prefix = me.map_or("!".to_string(), |m| m.prefix);
        ui.child_window("world_box").size([0.0, h]).build(ui, || {
            self.instances.draw(ui);
            ui.separator();
            self.forms.draw_page(
                ui,
                "Debug>World",
                &cmds,
                &[
                    "tp",
                    "stuck",
                    "time scale",
                    "instance new",
                    "instance transfer",
                    "instance join",
                    "instance close",
                    "instance reset",
                    "waypoint unlock",
                    "checkpoint unlock",
                ],
                &prefix,
            );
        });
    }

    fn players(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "Who is online, with kick, mute, teleport and summon; announcements and accounts.",
            "Chat equivalents: !who, !kick <player> [reason], !mute <player> [duration], !unmute \
             <player>, !tp <player>, !summon <player>, !announce <message>.",
        );
        ui.text("Announce");
        ui.same_line();
        ui.set_next_item_width(-90.0);
        ui.input_text("##announce", &mut self.announce).build();
        ui.same_line();
        if ui.button("Send") && !self.announce.trim().is_empty() {
            devlink::command(&format!("announce {}", self.announce.trim()));
            self.announce.clear();
        }
        ui.text("Mute for");
        ui.same_line();
        ui.set_next_item_width(80.0);
        ui.input_text("##mute_for", &mut self.mute_for).build();
        let h = (ui.content_region_avail()[1] - 120.0).max(160.0);
        let (cmds, me) = devlink::registry();
        let prefix = me.map_or("!".to_string(), |m| m.prefix);
        let online = devlink::online();
        let mute_for = self.mute_for.trim().to_string();
        let forms = &mut self.forms;
        ui.child_window("players_box").size([0.0, h]).build(ui, || {
            if online.is_empty() {
                widgets::empty(ui, "No list from the server yet (it comes every second).");
            } else if let Some(_t) = ui.begin_table_header_with_flags(
                "online_v1",
                [
                    "Character",
                    "Account",
                    "Level",
                    "League",
                    "Area",
                    "Instance",
                    "",
                ]
                .map(imgui::TableColumnSetup::new),
                super::table_opts(),
            ) {
                for o in &online {
                    let _id = ui.push_id(o.character.as_str());
                    for text in [
                        &o.character,
                        &o.account,
                        &o.level,
                        &o.league,
                        &o.area,
                        &o.instance,
                    ] {
                        ui.table_next_column();
                        ui.text(text);
                    }
                    ui.table_next_column();
                    let who = quote(&o.character);
                    if ui.small_button("Kick") {
                        devlink::command(&format!("player kick {who}"));
                    }
                    ui.same_line();
                    if o.muted {
                        if ui.small_button("Unmute") {
                            devlink::command(&format!("player unmute {who}"));
                        }
                    } else if ui.small_button("Mute") {
                        devlink::command(
                            &format!("player mute {who} {mute_for}").trim().to_string(),
                        );
                    }
                    ui.same_line();
                    if ui.small_button("TP to") {
                        devlink::command(&format!("tp {who}"));
                    }
                    ui.same_line();
                    if ui.small_button("Summon") {
                        devlink::command(&format!("summon {who}"));
                    }
                }
            }
            ui.separator();
            forms.draw_page(
                ui,
                "Debug>Players",
                &cmds,
                &["player kick", "player unmute", "summon", "announce"],
                &prefix,
            );
        });
    }

    fn character(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "Your character as the server sees it. Level, XP, points and class change here once \
             they exist.",
            "Comes with P10 (levels and XP) and P15 (passives).",
        );
        let value = |key: &str| devlink::live(key).unwrap_or_else(|| "-".into());
        ui.text(format!(
            "{}   level {}   class {}   league {}",
            value("character"),
            value("level"),
            value("class"),
            value("league")
        ));
        ui.text_disabled(format!(
            "{}   instance {}   cell {}",
            value("area"),
            value("instance"),
            value("cell")
        ));
        ui.separator();
        self.generated(ui, "Debug>Character");
    }

    /// A tab whose controls come with a later step: says which, and has the forms that exist.
    fn area_tab(&mut self, ui: &Ui, page: &str, what: &str, step: &str) {
        widgets::header(ui, what, "");
        ui.text_disabled(format!("Comes with {step}."));
        ui.separator();
        self.generated(ui, page);
    }

    fn generated(&mut self, ui: &Ui, page: &str) {
        let (cmds, me) = devlink::registry();
        let prefix = me.map_or("!".to_string(), |m| m.prefix);
        let h = (ui.content_region_avail()[1] - 120.0).max(120.0);
        let forms = &mut self.forms;
        ui.child_window("generated_box")
            .size([0.0, h])
            .build(ui, || {
                forms.draw_page(ui, page, &cmds, &[], &prefix);
            });
    }
}

/// The right pane of Help: usage, text, examples, aliases and the form.
fn help_pane(ui: &Ui, c: &Cmd, prefix: &str, forms: &mut CommandsView) {
    ui.text(format!("{prefix}{}", c.usage()));
    ui.text_wrapped(&c.summary);
    if c.help != c.summary {
        ui.text_wrapped(&c.help);
    }
    for f in &c.flags {
        ui.text_disabled(format!("--{}: {}", f.name, f.help));
    }
    if !c.examples.is_empty() {
        let list: Vec<String> = c.examples.iter().map(|e| format!("{prefix}{e}")).collect();
        ui.text_wrapped(format!("Examples: {}", list.join("   ")));
    }
    if !c.aliases.is_empty() {
        let list: Vec<String> = c.aliases.iter().map(|a| format!("{prefix}{a}")).collect();
        ui.text_wrapped(format!("Aliases: {}", list.join(" ")));
    }
    ui.text_disabled(format!(
        "Permission: {}   Overlay: {}",
        if c.admin { "admin" } else { "player" },
        c.overlay.as_deref().unwrap_or("-")
    ));
    ui.separator();
    forms.draw_command(ui, c, prefix);
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
