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
    /// Scroll the Help list to the selection once (it came from the palette).
    help_scroll: bool,
    console: String,
    /// The Console output follows new results unless scrolled up.
    console_follow: super::follow::Follow,
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
            help_scroll: false,
            console: String::new(),
            console_follow: super::follow::Follow::new(),
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

    /// Shows `name` on the Help tab with its form (the palette's "Form" rows).
    pub fn select_help(&mut self, name: &str) {
        self.help_filter.clear();
        self.help_sel = Some(name.to_string());
        self.help_scroll = true;
    }

    pub fn draw(&mut self, ui: &Ui, page: Page) {
        // The Console shows the results itself; a placeholder tab with no form needs none.
        let placeholder = match page {
            Page::DebugStats => Some("Debug>Stats"),
            Page::DebugItems => Some("Debug>Items"),
            Page::DebugMonsters => Some("Debug>Monsters"),
            Page::DebugLife => Some("Debug>Life"),
            _ => None,
        };
        let strip = page != Page::DebugConsole && placeholder.is_none_or(|p| self.has_forms(p));
        // The page above, a fixed strip of the last results below.
        let strip_h = results_height(ui);
        let body_h = if strip {
            -(strip_h + ui.frame_height_with_spacing())
        } else {
            0.0
        };
        // Wide rows (World's toolbar, long forms) scroll sideways in a narrow window.
        ui.child_window("debug_body")
            .size([0.0, body_h])
            .flags(imgui::WindowFlags::HORIZONTAL_SCROLLBAR)
            .build(ui, || self.body(ui, page));
        if strip {
            results_strip(ui, strip_h);
        }
    }

    fn body(&mut self, ui: &Ui, page: Page) {
        match page {
            Page::DebugHelp => self.help(ui),
            Page::DebugConsole => self.console(ui),
            Page::DebugWorld => self.world(ui),
            Page::DebugPlayers => self.players(ui),
            Page::DebugCharacter => self.character(ui),
            Page::DebugStats => {
                self.area_tab(ui, "Debug>Stats", "Stat editor", "P10, stats pipeline")
            }
            Page::DebugItems => self.area_tab(ui, "Debug>Items", "Item creator", "P12"),
            Page::DebugMonsters => {
                self.area_tab(ui, "Debug>Monsters", "Monster spawner and inspector", "P17")
            }
            Page::DebugLife => self.area_tab(
                ui,
                "Debug>Life",
                "Life, mana and energy shield",
                "P13, life, damage and death",
            ),
            _ => {}
        }
    }

    /// Whether the registry assigns any command to this tab.
    fn has_forms(&self, page: &str) -> bool {
        devlink::has_overlay_commands(page)
    }

    /// The registry, asking the server again now and then while it is empty.
    fn registry(&mut self, ui: &Ui) -> Option<(Vec<Cmd>, Option<devlink::Me>)> {
        let (cmds, me) = devlink::registry();
        if cmds.is_empty() {
            widgets::empty(
                ui,
                "No command list from the server yet (needs position sync and [dev] debug_link).",
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
        widgets::counter(ui, &widgets::plural(shown.len(), "command"));
        let mut groups: Vec<&str> = Vec::new();
        for c in &shown {
            if !groups.contains(&c.category.as_str()) {
                groups.push(&c.category);
            }
        }
        let sel = self.help_sel.clone();
        let scroll = std::mem::take(&mut self.help_scroll);
        let mut picked = None;
        let forms = &mut self.forms;
        ui.child_window("help_box").size([0.0, 0.0]).build(ui, || {
            widgets::split(
                ui,
                "debug_help",
                240.0,
                || {
                    if shown.is_empty() {
                        widgets::empty(ui, "No command matches.");
                    }
                    for g in &groups {
                        ui.text_disabled(g);
                        for c in shown.iter().filter(|c| c.category == *g) {
                            let name = c.name();
                            if ui
                                .selectable_config(format!("  {name}##h{name}"))
                                .selected(sel.as_deref() == Some(name.as_str()))
                                .build()
                            {
                                picked = Some(name.clone());
                            }
                            if scroll && sel.as_deref() == Some(name.as_str()) {
                                ui.set_scroll_here_y(0.5);
                            }
                        }
                    }
                },
                || match shown
                    .iter()
                    .find(|c| sel.as_deref() == Some(c.name().as_str()))
                {
                    Some(c) => help_pane(ui, c, &prefix, forms),
                    None if sel.is_some() && !q.is_empty() => {
                        widgets::empty(ui, "The selected command is filtered out.")
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
            "Up/Down recall earlier lines. Tab completes command names, subcommands, flags, \
             choices and names from the game data (the list comes from the server).",
        );
        let Some((cmds, me)) = self.registry(ui) else {
            return;
        };
        let prefix = me.as_ref().map_or("!".to_string(), |m| m.prefix.clone());
        if ui.small_button("Copy results") {
            let text: Vec<String> = devlink::results()
                .iter()
                .map(|(ms, cmd, result)| {
                    format!("{} {prefix}{cmd}\t{result}", crate::util::clock(*ms))
                })
                .collect();
            super::set_clipboard(&text.join("\n"));
        }
        let h = (ui.content_region_avail()[1] - 2.0 * ui.frame_height_with_spacing())
            .max(widgets::px(ui, 100.0));
        let follow = &mut self.console_follow;
        ui.child_window("console_out")
            .size([0.0, h])
            .border(true)
            .build(ui, || {
                let results = devlink::results();
                follow.begin(ui, results.len());
                if results.is_empty() {
                    widgets::empty(ui, "Nothing run yet.");
                }
                for (ms, cmd, result) in results.iter() {
                    ui.text_disabled(format!("{} > {prefix}{cmd}", crate::util::clock(*ms)));
                    let _wrap = ui.push_text_wrap_pos(0.0);
                    ui.text_colored(result_color(ui, result), format!("  {result}"));
                }
                follow.end(ui, results.len());
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
        ui.set_next_item_width(
            -ui.calc_text_size("Run")[0] - 3.0 * ui.clone_style().item_spacing()[0],
        );
        let enter = ui
            .input_text("##console", &mut self.console)
            .hint("command, e.g. help")
            .flags(imgui::InputTextFlags::ESCAPE_CLEARS_ALL)
            .enter_returns_true(true)
            .build();
        let active = ui.is_item_active() || enter;
        ui.same_line();
        let run = ui.button("Run");
        if self.console.is_empty() {
            self.hist_pos = None;
        }
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
        let mut take = None;
        if offered.is_empty() {
            ui.text_disabled("Tab completes; Up/Down recall; Escape clears.");
        } else {
            ui.text_disabled("Tab:");
        }
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
        let em = ui.current_font_size();
        widgets::label(ui, "Teleport to cell");
        ui.set_next_item_width(9.0 * em);
        let mut cell = [self.tx, self.ty];
        if ui.input_int2("##tp", &mut cell).build() {
            [self.tx, self.ty] = cell;
        }
        ui.same_line();
        if ui.button("Teleport") {
            devlink::command(&format!("tp {} {}", self.tx, self.ty));
        }
        ui.same_line();
        if ui.button("Use my cell")
            && let Some(p) = crate::inspect::player()
        {
            self.tx = p.grid.0;
            self.ty = p.grid.1;
        }
        if ui.is_item_hovered() {
            ui.tooltip_text("Fill in the cell you stand on.");
        }
        ui.same_line();
        if ui.button("Stuck") {
            devlink::command("stuck");
        }
        if ui.is_item_hovered() {
            ui.tooltip_text("Move to the nearest walkable cell (!stuck).");
        }
        widgets::label(ui, "Time scale");
        ui.set_next_item_width(5.0 * em);
        let apply = ui
            .input_text("##scale", &mut self.scale)
            .enter_returns_true(true)
            .build();
        ui.same_line();
        if ui.button("Apply##scale") || apply {
            devlink::command(&format!("time scale {}", self.scale.trim()));
        }
        ui.same_line();
        ui.text_disabled(format!(
            "now {}   seed {}",
            devlink::live("time_scale").unwrap_or_else(|| "-".into()),
            devlink::live("seed").unwrap_or_else(|| "-".into())
        ));
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
        if ui.is_item_hovered() {
            ui.tooltip_text("Write this area's layout and grids under the dump folder.");
        }
        widgets::label(ui, "Unlock");
        if ui.button("All acts") {
            devlink::command("waypoint unlock all");
        }
        ui.same_line();
        widgets::label(ui, "act");
        ui.set_next_item_width(6.0 * em);
        ui.input_int("##unlock_act", &mut self.unlock_act);
        if ui.is_item_hovered() {
            ui.tooltip_text("1-5, or 6 for the interludes.");
        }
        self.unlock_act = self.unlock_act.clamp(1, 6);
        ui.same_line();
        if ui.button("Unlock act") {
            devlink::command(&format!("waypoint unlock act {}", self.unlock_act));
        }
        ui.same_line();
        ui.set_next_item_width(11.0 * em);
        ui.input_text("##unlock_area", &mut self.unlock_area)
            .hint("area id, e.g. G1_4")
            .build();
        ui.same_line();
        {
            let _d = ui.begin_disabled_with_cond(self.unlock_area.trim().is_empty());
            if ui.button("Unlock waypoint") {
                devlink::command(&format!(
                    "waypoint unlock waypoint {}",
                    self.unlock_area.trim()
                ));
            }
        }
        ui.same_line();
        if ui.button("Checkpoints here") {
            devlink::command("checkpoint unlock all");
        }
        if ui.is_item_hovered() {
            ui.tooltip_text("Unlock every checkpoint in this instance.");
        }
        ui.separator();
        let h = ui.content_region_avail()[1].max(widgets::px(ui, 160.0));
        let (cmds, me) = devlink::registry();
        let prefix = me.map_or("!".to_string(), |m| m.prefix);
        ui.child_window("world_box")
            .size([0.0, h])
            .flags(imgui::WindowFlags::HORIZONTAL_SCROLLBAR)
            .build(ui, || {
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
        widgets::label(ui, "Announce");
        ui.set_next_item_width(-(ui.calc_text_size("Send")[0] + 3.0 * widgets::px(ui, 8.0)));
        let enter = ui
            .input_text("##announce", &mut self.announce)
            .hint("a message to everyone online")
            .flags(imgui::InputTextFlags::ESCAPE_CLEARS_ALL)
            .enter_returns_true(true)
            .build();
        ui.same_line();
        {
            let _d = ui.begin_disabled_with_cond(self.announce.trim().is_empty());
            if (ui.button("Send") || enter) && !self.announce.trim().is_empty() {
                devlink::command(&format!("announce {}", self.announce.trim()));
                self.announce.clear();
            }
        }
        widgets::label(ui, "Mute for");
        ui.set_next_item_width(widgets::px(ui, 80.0));
        ui.input_text("##mute_for", &mut self.mute_for)
            .hint("30m, 2h, 1d")
            .build();
        ui.same_line();
        ui.text_disabled("(used by Mute; empty = until unmuted by hand)");
        let h = ui.content_region_avail()[1].max(widgets::px(ui, 160.0));
        let (cmds, me) = devlink::registry();
        let prefix = me.map_or("!".to_string(), |m| m.prefix);
        let online = devlink::online();
        let mute_for = self.mute_for.trim().to_string();
        let forms = &mut self.forms;
        ui.child_window("players_box")
            .size([0.0, h])
            .flags(imgui::WindowFlags::HORIZONTAL_SCROLLBAR)
            .build(ui, || {
                if online.is_empty() {
                    widgets::empty(ui, "No list from the server yet (it comes every second).");
                } else if let Some(_t) = ui.begin_table_header_with_flags(
                    "online_v1",
                    [
                        "character",
                        "account",
                        "level",
                        "league",
                        "area",
                        "instance",
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
                            ui.open_popup("##kick");
                        }
                        if widgets::confirm_popup(
                            ui,
                            "##kick",
                            &format!("Kick {}? They can log in again.", o.character),
                            "Kick",
                        ) {
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
        widgets::header(
            ui,
            &format!("{what}: not built yet (comes with {step})."),
            "",
        );
        if self.has_forms(page) {
            ui.separator();
            self.generated(ui, page);
        }
    }

    fn generated(&mut self, ui: &Ui, page: &str) {
        let (cmds, me) = devlink::registry();
        let prefix = me.map_or("!".to_string(), |m| m.prefix);
        let h = ui.content_region_avail()[1].max(widgets::px(ui, 120.0));
        let forms = &mut self.forms;
        ui.child_window("generated_box")
            .size([0.0, h])
            .flags(imgui::WindowFlags::HORIZONTAL_SCROLLBAR)
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

/// The strip's height: five result lines.
fn results_height(ui: &Ui) -> f32 {
    5.0 * ui.text_line_height_with_spacing() + 2.0 * ui.clone_style().window_padding()[1]
}

/// The last 20 ack lines of any command, newest first, at the bottom of every Debug tab.
fn results_strip(ui: &Ui, h: f32) {
    ui.text_disabled("Results, newest first");
    ui.child_window("results")
        .size([0.0, h])
        .border(true)
        .build(ui, || {
            let all = devlink::results();
            if all.is_empty() {
                widgets::empty(ui, "No command run yet.");
            }
            for (ms, cmd, result) in all.iter().rev().take(20) {
                let _wrap = ui.push_text_wrap_pos(0.0);
                ui.text_colored(
                    result_color(ui, result),
                    format!("{} {cmd} -> {result}", crate::util::clock(*ms)),
                );
            }
        });
}

/// `ok: ...` green, `error: ...` red, anything else plain (the server's ack status).
fn result_color(ui: &Ui, result: &str) -> [f32; 4] {
    if result.starts_with("ok") {
        super::theme::OK
    } else if result.starts_with("error") {
        super::theme::BAD
    } else {
        ui.style_color(imgui::StyleColor::Text)
    }
}
