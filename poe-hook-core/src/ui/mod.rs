//! The overlay's windows: the main debug window with its tabs, the read-only badge and
//! short notices. Each tab keeps its own state in [`App`].

mod alert;
mod commands_tab;
mod coverage_tab;
mod debug_tab;
mod follow;
mod game_tab;
mod hooks_tab;
mod inspect_tab;
mod instances_tab;
mod layout_form;
mod log_tab;
mod notes_tab;
mod packets_tab;
mod protocol_tab;
mod scanner_tab;
mod server_tab;
pub mod settings;
mod settings_tab;
mod status_tab;
pub mod theme;
mod timing_tab;
pub mod widgets;

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use imgui::{Condition, Ui, WindowFlags};

static NOTICES: Mutex<Vec<(String, Instant)>> = Mutex::new(Vec::new());
const NOTICE_TIME: Duration = Duration::from_secs(5);

/// A short message shown at the top of the screen for a few seconds (also logged).
pub fn notice(text: String) {
    log::info!("{text}");
    NOTICES.lock().unwrap().push((text, Instant::now()));
}

pub use alert::{Alert, Row as AlertRow, alert, has_alerts};

pub fn has_notices() -> bool {
    let mut n = NOTICES.lock().unwrap();
    n.retain(|(_, t)| t.elapsed() < NOTICE_TIME);
    !n.is_empty()
}

/// Whether features that talk to our server are shown: never while the read-only latch is on
/// (an official server), and only while every connection the game made is trusted (loopback
/// or `trusted_servers`, `poe_hook_shared::trust`), so nothing in the overlay can send to an
/// official server. The latch already covers the second condition; it is checked again here.
pub fn custom_server() -> bool {
    !crate::host::read_only() && crate::host::connections().iter().all(|c| c.trusted())
}

/// Plain-text summary of the client state (snapshots).
pub fn state_text() -> String {
    status_tab::state_text()
}

pub struct App {
    log: log_tab::LogView,
    packets: packets_tab::PacketsView,
    coverage: coverage_tab::CoverageView,
    notes: notes_tab::NotesView,
    timing: timing_tab::TimingView,
    entities: inspect_tab::EntitiesView,
    watches: inspect_tab::WatchesView,
    hooks: hooks_tab::HooksView,
    protocol: protocol_tab::ProtocolView,
    scanner: scanner_tab::ScannerView,
    stats: game_tab::StatsView,
    actions: game_tab::ActionsView,
    server: server_tab::ServerView,
    debug: debug_tab::DebugView,
    settings: settings_tab::SettingsView,
    page: Page,
    /// The sub-page last shown per sidebar entry (an entry with tabs opens on it again).
    last_tab: Vec<(&'static str, Page)>,
    page_search: String,
    palette_query: String,
    /// The palette row Up/Down picked (Enter takes it).
    palette_sel: usize,
    /// `POE_HOOK_OVERLAY_TOUR` (game tests): shows every page in turn and snapshots each.
    tour: Option<Tour>,
}

/// The overlay tour: the page index, when it was shown, and whether its snapshot was taken.
struct Tour {
    index: usize,
    since: Instant,
    shot: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            settings: settings_tab::SettingsView::new(),
            log: log_tab::LogView::new(),
            packets: packets_tab::PacketsView::new(),
            coverage: coverage_tab::CoverageView::new(),
            notes: notes_tab::NotesView::new(),
            timing: timing_tab::TimingView::new(),
            entities: inspect_tab::EntitiesView::new(),
            watches: inspect_tab::WatchesView::new(),
            hooks: hooks_tab::HooksView::new(),
            protocol: protocol_tab::ProtocolView::new(),
            scanner: scanner_tab::ScannerView::new(),
            stats: game_tab::StatsView::new(),
            actions: game_tab::ActionsView::new(),
            server: server_tab::ServerView::new(),
            debug: debug_tab::DebugView::new(),
            page: load_page(),
            last_tab: Vec::new(),
            page_search: String::new(),
            palette_query: String::new(),
            palette_sel: 0,
            tour: std::env::var_os("POE_HOOK_OVERLAY_TOUR").map(|_| Tour {
                index: usize::MAX,
                since: Instant::now(),
                shot: true,
            }),
        }
    }

    pub fn draw(&mut self, ui: &Ui, visible: bool, plots: &dear_implot::PlotContext) {
        // Logs are pulled every drawn frame so nothing is missed while hidden.
        self.log.pull();
        self.scanner.update();
        if crate::host::read_only() {
            badge(ui);
        }
        notices(ui);
        alert::draw(ui);
        if !visible {
            return;
        }
        self.tour_step();
        // First use: about 60% of a 1440p screen at any scale (later sizes come from imgui.ini).
        let em = ui.current_font_size();
        ui.window("poe-hook##main2")
            .size([95.0 * em, 50.0 * em], Condition::FirstUseEver)
            .position([40.0, 80.0], Condition::FirstUseEver)
            .build(|| {
                self.palette(ui);
                let avail = ui.content_region_avail();
                ui.child_window("nav")
                    .size([14.0 * ui.current_font_size(), avail[1]])
                    .border(true)
                    .build(ui, || {
                        self.nav(ui);
                    });
                ui.same_line();
                ui.child_window("page").size([0.0, avail[1]]).build(ui, || {
                    self.draw_page(ui, plots);
                });
            });
    }

    /// One step of the overlay tour: every 3 s the next page, with a snapshot 1.5 s after it
    /// is shown. The log lines `overlay tour: <Page>` and `overlay tour: done` let the test
    /// pair each page with its screenshot.
    fn tour_step(&mut self) {
        let Some(t) = self.tour.as_mut() else {
            return;
        };
        let all: Vec<Page> = pages().map(|(_, p, _)| p).filter(|p| visible(*p)).collect();
        if t.index != usize::MAX && t.index >= all.len() {
            return;
        }
        if !t.shot && t.since.elapsed() >= Duration::from_millis(1500) {
            t.shot = true;
            log::info!("overlay tour: {:?}", all[t.index]);
            crate::snapshot::request();
        } else if t.shot && t.since.elapsed() >= Duration::from_secs(3) {
            let next = t.index.wrapping_add(1);
            t.index = next;
            t.since = Instant::now();
            t.shot = false;
            match all.get(next) {
                Some(p) => {
                    let p = *p;
                    self.go(p);
                }
                None => log::info!("overlay tour: done"),
            }
        }
    }

    /// The sidebar: groups (folding headers; Research starts folded) and their entries (our
    /// server's group hidden while read-only). With text in the search box: the matching pages.
    fn nav(&mut self, ui: &Ui) {
        ui.set_next_item_width(crate::ui::widgets::px(ui, -1.0));
        let enter = ui
            .input_text("##page_search", &mut self.page_search)
            .hint("search pages")
            .flags(imgui::InputTextFlags::ESCAPE_CLEARS_ALL)
            .enter_returns_true(true)
            .build();
        ui.text_disabled("Ctrl+P: palette");
        let f = self.page_search.trim().to_lowercase();
        if !f.is_empty() {
            let found: Vec<(Page, String)> = pages()
                .filter(|(e, p, n)| {
                    visible(*p) && (n.to_lowercase().contains(&f) || e.to_lowercase().contains(&f))
                })
                .map(|(e, p, n)| {
                    (
                        p,
                        if e == n {
                            n.to_string()
                        } else {
                            format!("{e} / {n}")
                        },
                    )
                })
                .collect();
            ui.spacing();
            if found.is_empty() {
                widgets::empty(ui, "No page matches.");
            }
            let mut take = if enter {
                found.first().map(|f| f.0)
            } else {
                None
            };
            for (page, name) in &found {
                if ui
                    .selectable_config(format!("{name}##ps{page:?}"))
                    .selected(self.page == *page)
                    .build()
                {
                    take = Some(*page);
                }
            }
            // Opening a result ends the search, so the sidebar shows the groups again.
            if let Some(page) = take {
                self.go(page);
                self.page_search.clear();
            }
            return;
        }
        // Group headers are quiet bars, so the selected page stands out, not the groups.
        let stock = ui.clone_style();
        let _h = ui.push_style_color(imgui::StyleColor::Header, [0.20, 0.22, 0.27, 0.55]);
        let _hh = ui.push_style_color(imgui::StyleColor::HeaderHovered, [0.26, 0.29, 0.36, 0.80]);
        let _ha = ui.push_style_color(imgui::StyleColor::HeaderActive, [0.30, 0.34, 0.42, 1.00]);
        for g in GROUPS {
            if g.custom && !custom_server() {
                continue;
            }
            ui.spacing();
            let here = g
                .entries
                .iter()
                .any(|e| e.pages.iter().any(|(p, _)| *p == self.page));
            // The user's folds come from overlay.toml; the group of the page shown starts open.
            let folded = settings::folded(g.name);
            ui.set_next_item_open_with_cond(here || !folded, Condition::Once);
            let open = ui.collapsing_header(g.name, imgui::TreeNodeFlags::empty());
            if ui.is_item_toggled_open() {
                settings::update(|s| {
                    s.sidebar_folded.retain(|n| n != g.name);
                    if !open {
                        s.sidebar_folded.push(g.name.to_string());
                    }
                });
            }
            if !open {
                continue;
            }
            let _eh = ui.push_style_color(
                imgui::StyleColor::Header,
                stock.color(imgui::StyleColor::Header),
            );
            let _ehh = ui.push_style_color(
                imgui::StyleColor::HeaderHovered,
                stock.color(imgui::StyleColor::HeaderHovered),
            );
            let _eha = ui.push_style_color(
                imgui::StyleColor::HeaderActive,
                stock.color(imgui::StyleColor::HeaderActive),
            );
            for e in g.entries {
                let selected = e.pages.iter().any(|(p, _)| *p == self.page);
                if ui
                    .selectable_config(format!("  {}##nav", e.name))
                    .selected(selected)
                    .build()
                    && !selected
                {
                    let last = self
                        .last_tab
                        .iter()
                        .find(|(n, _)| *n == e.name)
                        .map(|(_, p)| *p);
                    self.go(last.unwrap_or(e.pages[0].0));
                }
            }
        }
    }

    fn go(&mut self, page: Page) {
        self.page = page;
        if let Some(e) = entry_of(page) {
            match self.last_tab.iter_mut().find(|(n, _)| *n == e.name) {
                Some(slot) => slot.1 = page,
                None => self.last_tab.push((e.name, page)),
            }
        }
        save_page(page);
    }

    /// For an entry with several pages: its tabs (Ctrl+PageUp / Ctrl+PageDown cycle them).
    fn page_tabs(&mut self, ui: &Ui) {
        let Some(e) = entry_of(self.page) else {
            return;
        };
        let tabs: Vec<(Page, &str)> = e
            .pages
            .iter()
            .filter(|(p, _)| visible(*p))
            .map(|(p, n)| (*p, *n))
            .collect();
        if tabs.len() <= 1 {
            return;
        }
        let at = tabs.iter().position(|(p, _)| *p == self.page).unwrap_or(0);
        let io = ui.io();
        if io.key_ctrl() && !io.want_text_input() {
            if ui.is_key_pressed(imgui::Key::PageDown) {
                self.go(tabs[(at + 1) % tabs.len()].0);
            } else if ui.is_key_pressed(imgui::Key::PageUp) {
                self.go(tabs[(at + tabs.len() - 1) % tabs.len()].0);
            }
        }
        let pad = ui.current_font_size();
        let _align = ui.push_style_var(imgui::StyleVar::SelectableTextAlign([0.5, 0.5]));
        for (i, (page, name)) in tabs.iter().enumerate() {
            if i > 0 {
                ui.same_line();
            }
            let on = *page == self.page;
            let w = ui.calc_text_size(name)[0] + 2.0 * pad;
            if ui
                .selectable_config(format!("{name}##tab"))
                .selected(on)
                .size([w, 0.0])
                .build()
                && !on
            {
                self.go(*page);
            }
            if ui.is_item_hovered() && i == 0 {
                ui.tooltip_text("Ctrl+PageUp / Ctrl+PageDown: previous / next tab");
            }
        }
        ui.separator();
    }

    fn draw_page(&mut self, ui: &Ui, plots: &dear_implot::PlotContext) {
        // A page of our server only, while connected to an official one: back home.
        if !visible(self.page) {
            self.page = if page_is_custom(self.page) && custom_server() {
                Page::DebugHelp
            } else {
                Page::Status
            };
        }
        self.page_tabs(ui);
        match self.page {
            Page::Status => status_tab::draw(ui),
            Page::Log => self.log.draw(ui),
            Page::DebugHelp
            | Page::DebugConsole
            | Page::DebugCharacter
            | Page::DebugStats
            | Page::DebugItems
            | Page::DebugWorld
            | Page::DebugMonsters
            | Page::DebugLife
            | Page::DebugPlayers => self.debug.draw(ui, self.page),
            Page::Sync => self.server.draw(ui, plots),
            Page::Packets => self.packets.draw(ui),
            Page::Protocol => self.protocol.draw(ui),
            Page::Coverage => self.coverage.draw(ui),
            Page::Notes => self.notes.draw(ui),
            Page::Timing => self.timing.draw(ui, plots),
            Page::Player => inspect_tab::draw_player(ui),
            Page::Stats => self.stats.draw(ui),
            Page::Actions => self.actions.draw(ui),
            Page::Entities => self.entities.draw(ui),
            Page::Watches => self.watches.draw(ui),
            Page::Scanner => self.scanner.draw(ui),
            Page::Hooks => self.hooks.draw(ui),
            Page::Settings => self.settings.draw(ui),
        }
    }

    /// Ctrl+P: jump to a page, or run a server command line (`transfer G1_6 ...`). Up/Down
    /// pick a row, Enter takes it, Escape closes.
    fn palette(&mut self, ui: &Ui) {
        if ui.io().key_ctrl() && ui.is_key_pressed(imgui::Key::P) {
            self.palette_query.clear();
            self.palette_sel = 0;
            ui.open_popup("##palette");
        }
        let Some(_p) = ui.begin_popup("##palette") else {
            return;
        };
        let em = ui.current_font_size();
        if ui.is_window_appearing() {
            let [w, _] = ui.io().display_size();
            let (pw, ph) = (46.0 * em, 32.0 * em);
            ui.set_window_pos([(w - pw) * 0.5, 9.0 * em]);
            ui.set_window_size([pw, ph]);
            ui.set_keyboard_focus_here();
        }
        ui.set_next_item_width(crate::ui::widgets::px(ui, -1.0));
        let before = self.palette_query.clone();
        let enter = ui
            .input_text("##palette_query", &mut self.palette_query)
            .hint("page name, or a server command line")
            .flags(imgui::InputTextFlags::ESCAPE_CLEARS_ALL)
            .enter_returns_true(true)
            .build();
        if self.palette_query != before {
            self.palette_sel = 0;
        }
        ui.text_disabled("Up/Down: pick   Enter: open or run   Escape: close");
        ui.separator();
        let items = self.palette_items();
        if ui.is_key_pressed(imgui::Key::DownArrow) && !items.is_empty() {
            self.palette_sel = (self.palette_sel + 1).min(items.len() - 1);
        }
        if ui.is_key_pressed(imgui::Key::UpArrow) {
            self.palette_sel = self.palette_sel.saturating_sub(1);
        }
        self.palette_sel = self.palette_sel.min(items.len().saturating_sub(1));
        let mut chosen: Option<usize> = None;
        ui.child_window("##palette_rows").build(ui, || {
            if items.is_empty() {
                widgets::empty(ui, "Nothing matches: try a page name or a command.");
            }
            for (i, (label, _)) in items.iter().enumerate() {
                let on = i == self.palette_sel;
                if ui
                    .selectable_config(format!("{label}##pal{i}"))
                    .selected(on)
                    .build()
                {
                    chosen = Some(i);
                }
                if on
                    && (ui.is_key_pressed(imgui::Key::DownArrow)
                        || ui.is_key_pressed(imgui::Key::UpArrow))
                {
                    ui.set_scroll_here_y(0.5);
                }
            }
        });
        if enter && !items.is_empty() {
            chosen = Some(self.palette_sel);
        }
        if ui.is_key_pressed(imgui::Key::Escape) {
            ui.close_current_popup();
            return;
        }
        let Some(i) = chosen else {
            return;
        };
        match items.into_iter().nth(i).map(|(_, item)| item) {
            Some(PaletteItem::Page(p)) => self.go(p),
            // A bare command that needs arguments: open its form instead of running it.
            Some(PaletteItem::Command(_, true, name)) => {
                self.debug.select_help(&name);
                self.go(Page::DebugHelp);
            }
            Some(PaletteItem::Command(line, false, _)) => {
                crate::devlink::command(&line);
            }
            None => {}
        }
        ui.close_current_popup();
    }

    /// The palette's rows for the current query: matching pages, then matching commands.
    fn palette_items(&self) -> Vec<(String, PaletteItem)> {
        let q = self.palette_query.trim().to_lowercase();
        let first_word = q.split_whitespace().next().unwrap_or("").to_string();
        let has_args = q.contains(' ');
        let mut items = Vec::new();
        if !has_args {
            for (entry, page, name) in pages() {
                if !visible(page) {
                    continue;
                }
                if !q.is_empty()
                    && !name.to_lowercase().contains(&q)
                    && !entry.to_lowercase().contains(&q)
                {
                    continue;
                }
                let label = if entry == name {
                    format!("Page   {name}")
                } else {
                    format!("Page   {entry} / {name}")
                };
                items.push((label, PaletteItem::Page(page)));
            }
        }
        if custom_server() {
            let (cmds, me) = crate::devlink::registry();
            let admin = me.is_some_and(|m| m.admin);
            for c in cmds.iter().filter(|c| admin || !c.admin) {
                let name = c.name();
                // The typed line is run as is only when its first word(s) name this command
                // exactly (name or alias); otherwise the row runs the bare command.
                let typed_name = q == name || q.starts_with(&format!("{name} "));
                let typed_alias = c.aliases.iter().any(|a| *a == first_word);
                let matches = q.is_empty()
                    || typed_name
                    || typed_alias
                    || (!has_args
                        && (name.starts_with(&q)
                            || name.contains(&q)
                            || c.aliases.iter().any(|a| a.starts_with(&q))));
                if !matches {
                    continue;
                }
                let line = if typed_name || typed_alias {
                    self.palette_query.trim().to_string()
                } else {
                    name.clone()
                };
                let needs_args = !line.contains(' ') && c.args.iter().any(|a| !a.optional);
                let what = if needs_args { "Form" } else { "Run " };
                items.push((
                    format!("{what}   {}  - {}", c.usage(), c.summary),
                    PaletteItem::Command(line, needs_args, name.clone()),
                ));
            }
        }
        items
    }
}

enum PaletteItem {
    Page(Page),
    /// The command line to run, whether it still needs arguments, and the command's name.
    Command(String, bool, String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Status,
    /// Player > Overview.
    Player,
    Stats,
    Actions,
    Entities,
    Watches,
    DebugHelp,
    DebugConsole,
    DebugCharacter,
    DebugStats,
    DebugItems,
    DebugWorld,
    DebugMonsters,
    DebugLife,
    DebugPlayers,
    Sync,
    Log,
    /// Packets > Messages.
    Packets,
    /// Packets > Opcodes.
    Coverage,
    Notes,
    Protocol,
    Timing,
    Scanner,
    Hooks,
    Settings,
}

/// A sidebar entry: its pages show as tabs when there are several.
struct Entry {
    name: &'static str,
    pages: &'static [(Page, &'static str)],
}

/// A sidebar group: a folding header.
struct Group {
    name: &'static str,
    /// Pages of our server only (hidden while read-only).
    custom: bool,
    entries: &'static [Entry],
}

const GROUPS: &[Group] = &[
    Group {
        name: "PLAY",
        custom: false,
        entries: &[
            Entry {
                name: "Status",
                pages: &[(Page::Status, "Status")],
            },
            Entry {
                name: "Player",
                pages: &[
                    (Page::Player, "Overview"),
                    (Page::Stats, "Stats"),
                    (Page::Actions, "Actions"),
                ],
            },
            Entry {
                name: "Entities",
                pages: &[(Page::Entities, "Entities"), (Page::Watches, "Watches")],
            },
        ],
    },
    Group {
        name: "DEBUG",
        custom: true,
        entries: &[
            Entry {
                name: "Debug",
                pages: &[
                    (Page::DebugHelp, "Help"),
                    (Page::DebugConsole, "Console"),
                    (Page::DebugCharacter, "Character"),
                    (Page::DebugStats, "Stats"),
                    (Page::DebugItems, "Items"),
                    (Page::DebugWorld, "World"),
                    (Page::DebugMonsters, "Monsters"),
                    (Page::DebugLife, "Life"),
                    (Page::DebugPlayers, "Players"),
                ],
            },
            Entry {
                name: "Sync",
                pages: &[(Page::Sync, "Sync")],
            },
        ],
    },
    Group {
        name: "LOGS",
        custom: false,
        entries: &[Entry {
            name: "Log",
            pages: &[(Page::Log, "Log")],
        }],
    },
    Group {
        name: "RESEARCH",
        custom: false,
        entries: &[
            Entry {
                name: "Packets",
                pages: &[
                    (Page::Packets, "Messages"),
                    (Page::Coverage, "Opcodes"),
                    (Page::Notes, "Notes"),
                ],
            },
            Entry {
                name: "Protocol",
                pages: &[(Page::Protocol, "Protocol")],
            },
            Entry {
                name: "Timing",
                pages: &[(Page::Timing, "Timing")],
            },
            Entry {
                name: "RE tools",
                pages: &[(Page::Scanner, "Scanner"), (Page::Hooks, "Hooks")],
            },
        ],
    },
    Group {
        name: "SETTINGS",
        custom: false,
        entries: &[Entry {
            name: "Settings",
            pages: &[(Page::Settings, "Settings")],
        }],
    },
];

/// Every page with its entry's name and its own name, in sidebar order.
fn pages() -> impl Iterator<Item = (&'static str, Page, &'static str)> {
    GROUPS
        .iter()
        .flat_map(|g| g.entries.iter())
        .flat_map(|e| e.pages.iter().map(move |(p, n)| (e.name, *p, *n)))
}

fn entry_of(page: Page) -> Option<&'static Entry> {
    GROUPS
        .iter()
        .flat_map(|g| g.entries.iter())
        .find(|e| e.pages.iter().any(|(p, _)| *p == page))
}

fn page_is_custom(page: Page) -> bool {
    GROUPS.iter().any(|g| {
        g.custom
            && g.entries
                .iter()
                .any(|e| e.pages.iter().any(|(p, _)| *p == page))
    })
}

/// Debug tabs only admins see: every one but Help and Console (hidden, not greyed out, once
/// the server has said the account is not an admin).
fn admin_only(page: Page) -> bool {
    matches!(
        page,
        Page::DebugCharacter
            | Page::DebugStats
            | Page::DebugItems
            | Page::DebugWorld
            | Page::DebugMonsters
            | Page::DebugLife
            | Page::DebugPlayers
    )
}

/// Whether the page may be shown now: our server's pages only while on our server, and the
/// admin-only tabs only for admins.
fn visible(page: Page) -> bool {
    if page_is_custom(page) && !custom_server() {
        return false;
    }
    !(admin_only(page) && crate::devlink::not_admin())
}

fn save_page(page: Page) {
    settings::update(|s| s.last_page = format!("{page:?}"));
}

/// The page selected last time (kept across sessions in `overlay.toml`).
fn load_page() -> Page {
    let name = settings::get().last_page;
    pages()
        .map(|(_, p, _)| p)
        .find(|p| format!("{p:?}") == name.trim())
        .unwrap_or(Page::Status)
}

/// Table options for the overlay's tables: columns fit their contents, resizable, striped.
/// A table that fills the rest of its pane and scrolls under a frozen header row (the long
/// lists). The `stretch` column takes the spare width, so long names are not cut.
pub fn list_table<'ui, Name: AsRef<str>, const N: usize>(
    ui: &'ui Ui,
    id: &str,
    columns: [imgui::TableColumnSetup<Name>; N],
    stretch: &str,
) -> Option<imgui::TableToken<'ui>> {
    let t = ui.begin_table_with_flags(
        id,
        N,
        table_opts().flags(
            imgui::TableFlags::RESIZABLE
                | imgui::TableFlags::ROW_BG
                | imgui::TableFlags::BORDERS_INNER_V
                | imgui::TableFlags::SCROLL_Y,
        ),
    )?;
    ui.table_setup_scroll_freeze(0, 1);
    for c in &columns {
        let width = if c.name.as_ref() == stretch {
            Some(imgui::TableColumnWidth::Stretch(1.0))
        } else {
            c.width
        };
        ui.table_setup_column(&c.name, c.flags, width);
    }
    ui.table_headers_row();
    Some(t)
}

pub fn table_opts() -> imgui::TableOptions {
    imgui::TableOptions::from(imgui::TableFlags::RESIZABLE | imgui::TableFlags::ROW_BG)
        .sizing_policy(imgui::TableSizingPolicy::FixedFit)
}

/// A Windows virtual-key code as its key's name (`0x2d` is "Insert").
pub fn key_name(vk: u16) -> String {
    match vk {
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x13 => "Pause".into(),
        0x14 => "Caps Lock".into(),
        0x20 => "Space".into(),
        0x21 => "Page Up".into(),
        0x22 => "Page Down".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x2c => "Print Screen".into(),
        0x2d => "Insert".into(),
        0x2e => "Delete".into(),
        0x30..=0x39 | 0x41..=0x5a => char::from(vk as u8).to_string(),
        0x60..=0x69 => format!("Numpad {}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6f),
        0x91 => "Scroll Lock".into(),
        0xc0 => "`".into(),
        _ => format!("key {vk:#04x}"),
    }
}

/// Shows a memory region in the Scanner tab.
pub fn scanner_request(expr: String, len: usize) {
    scanner_tab::request(expr, len);
}

/// Copies text to the system clipboard.
pub fn set_clipboard(text: &str) {
    let c = std::ffi::CString::new(text.replace('\0', "?")).unwrap_or_default();
    unsafe { imgui::sys::igSetClipboardText(c.as_ptr()) };
}

/// Top-left position that centres a one-line window of `text` horizontally.
fn centred(ui: &Ui, text: &str, y: f32) -> [f32; 2] {
    let [w, _] = ui.io().display_size();
    let tw = ui.calc_text_size(text)[0];
    [(w - tw) * 0.5 - 8.0, y]
}

const BADGE: &str = "OFFICIAL: read-only";

fn badge(ui: &Ui) {
    ui.window("##read-only")
        .position(centred(ui, BADGE, 6.0), Condition::Always)
        .flags(
            WindowFlags::NO_DECORATION
                | WindowFlags::NO_INPUTS
                | WindowFlags::ALWAYS_AUTO_RESIZE
                | WindowFlags::NO_SAVED_SETTINGS
                | WindowFlags::NO_FOCUS_ON_APPEARING
                | WindowFlags::NO_NAV,
        )
        .bg_alpha(0.75)
        .build(|| {
            ui.text_colored(theme::WARN, BADGE);
        });
}

fn notices(ui: &Ui) {
    let list: Vec<String> = {
        let mut n = NOTICES.lock().unwrap();
        n.retain(|(_, t)| t.elapsed() < NOTICE_TIME);
        n.iter().map(|(s, _)| s.clone()).collect()
    };
    if list.is_empty() {
        return;
    }
    let longest = list
        .iter()
        .max_by_key(|s| s.len())
        .cloned()
        .unwrap_or_default();
    ui.window("##notices")
        .position(centred(ui, &longest, 44.0), Condition::Always)
        .flags(
            WindowFlags::NO_DECORATION
                | WindowFlags::NO_INPUTS
                | WindowFlags::ALWAYS_AUTO_RESIZE
                | WindowFlags::NO_SAVED_SETTINGS
                | WindowFlags::NO_FOCUS_ON_APPEARING
                | WindowFlags::NO_NAV,
        )
        .bg_alpha(0.8)
        .build(|| {
            for s in &list {
                ui.text(s);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_keys_have_names() {
        assert_eq!(key_name(0x2d), "Insert");
        assert_eq!(key_name(0x24), "Home");
        assert_eq!(key_name(0x70), "F1");
        assert_eq!(key_name(0x41), "A");
        assert_eq!(key_name(0xff), "key 0xff");
    }

    #[test]
    fn every_page_is_in_the_sidebar_once() {
        let all = [
            Page::Status,
            Page::Player,
            Page::Stats,
            Page::Actions,
            Page::Entities,
            Page::Watches,
            Page::DebugHelp,
            Page::DebugConsole,
            Page::DebugCharacter,
            Page::DebugStats,
            Page::DebugItems,
            Page::DebugWorld,
            Page::DebugMonsters,
            Page::DebugLife,
            Page::DebugPlayers,
            Page::Sync,
            Page::Log,
            Page::Packets,
            Page::Coverage,
            Page::Notes,
            Page::Protocol,
            Page::Timing,
            Page::Scanner,
            Page::Hooks,
            Page::Settings,
        ];
        for p in all {
            assert_eq!(pages().filter(|(_, q, _)| *q == p).count(), 1, "{p:?}");
        }
        assert_eq!(pages().count(), all.len());
    }

    #[test]
    fn groups_fold_and_hide_as_agreed() {
        assert!(GROUPS.iter().any(|g| g.name == "RESEARCH"));
        assert!(
            settings::Settings::default()
                .sidebar_folded
                .contains(&"RESEARCH".to_string())
        );
        assert_eq!(entry_of(Page::Settings).unwrap().name, "Settings");
        assert!(page_is_custom(Page::Sync));
        assert!(page_is_custom(Page::DebugWorld));
        assert!(!page_is_custom(Page::Log));
    }

    #[test]
    fn merged_pages_share_one_entry() {
        assert_eq!(entry_of(Page::Stats).unwrap().name, "Player");
        assert_eq!(entry_of(Page::Actions).unwrap().name, "Player");
        assert_eq!(entry_of(Page::Notes).unwrap().name, "Packets");
        assert_eq!(entry_of(Page::Coverage).unwrap().name, "Packets");
        assert_eq!(entry_of(Page::DebugConsole).unwrap().name, "Debug");
    }

    #[test]
    fn only_help_and_console_are_open_to_everyone() {
        assert!(!admin_only(Page::DebugHelp));
        assert!(!admin_only(Page::DebugConsole));
        assert!(admin_only(Page::DebugPlayers));
        assert!(admin_only(Page::DebugWorld));
    }
}
