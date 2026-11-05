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
        let toggle = crate::config().overlay.toggle_key;
        ui.window("poe-hook##main2")
            .size([1500.0, 780.0], Condition::FirstUseEver)
            .position([40.0, 80.0], Condition::FirstUseEver)
            .build(|| {
                self.palette(ui);
                let avail = ui.content_region_avail();
                ui.child_window("nav")
                    .size([180.0, avail[1]])
                    .border(true)
                    .build(ui, || {
                        self.nav(ui);
                    });
                ui.same_line();
                ui.child_window("page").size([0.0, avail[1]]).build(ui, || {
                    self.draw_page(ui, plots);
                });
            });
        let _ = toggle;
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
        ui.set_next_item_width(-1.0);
        ui.input_text("##page_search", &mut self.page_search)
            .hint("search pages")
            .build();
        ui.text_disabled("Ctrl+P: palette");
        let f = self.page_search.to_lowercase();
        if !f.is_empty() {
            let found: Vec<(Page, &str)> = pages()
                .filter(|(_, p, n)| {
                    visible(*p)
                        && (n.to_lowercase().contains(&f)
                            || format!("{p:?}").to_lowercase().contains(&f))
                })
                .map(|(_, p, n)| (p, n))
                .collect();
            ui.spacing();
            for (page, name) in found {
                if ui
                    .selectable_config(name)
                    .selected(self.page == page)
                    .build()
                {
                    self.go(page);
                }
            }
            return;
        }
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
            let folded = settings::get().sidebar_folded.iter().any(|n| n == g.name);
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

    /// For an entry with several pages: its tabs.
    fn page_tabs(&mut self, ui: &Ui) {
        let Some(e) = entry_of(self.page) else {
            return;
        };
        if e.pages.iter().filter(|(p, _)| visible(*p)).count() > 1 {
            let mut first = true;
            for (page, name) in e.pages.iter().filter(|(p, _)| visible(*p)) {
                if !first {
                    ui.same_line();
                }
                first = false;
                let on = *page == self.page;
                let w = ui.calc_text_size(name)[0] + 20.0;
                if ui
                    .selectable_config(format!("{name}##tab"))
                    .selected(on)
                    .size([w, 0.0])
                    .build()
                    && !on
                {
                    self.go(*page);
                }
            }
            ui.separator();
        }
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

    /// Ctrl+P: jump to a page, or run a server command line (`transfer G1_6 ...`).
    fn palette(&mut self, ui: &Ui) {
        if ui.io().key_ctrl() && ui.is_key_pressed(imgui::Key::P) {
            self.palette_query.clear();
            ui.open_popup("##palette");
        }
        let Some(_p) = ui.begin_popup("##palette") else {
            return;
        };
        if ui.is_window_appearing() {
            let [w, _] = ui.io().display_size();
            ui.set_window_pos([w * 0.5 - 300.0, 120.0]);
            ui.set_window_size([600.0, 420.0]);
            ui.set_keyboard_focus_here();
        }
        ui.set_next_item_width(-1.0);
        let enter = ui
            .input_text("##palette_query", &mut self.palette_query)
            .hint("page name, or a server command line")
            .enter_returns_true(true)
            .build();
        let q = self.palette_query.trim().to_lowercase();
        let first_word = q.split_whitespace().next().unwrap_or("").to_string();
        let mut chosen: Option<PaletteItem> = None;
        let mut first: Option<PaletteItem> = None;
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
            first.get_or_insert(PaletteItem::Page(page));
            let label = if entry == name {
                format!("Page: {name}")
            } else {
                format!("Page: {entry} / {name}")
            };
            if ui.selectable(label) {
                chosen = Some(PaletteItem::Page(page));
            }
        }
        if custom_server() {
            let (cmds, me) = crate::devlink::registry();
            let admin = me.is_some_and(|m| m.admin);
            for c in cmds.iter().filter(|c| admin || !c.admin) {
                let name = c.name();
                let alias = c.aliases.iter().any(|a| a.starts_with(&first_word));
                if !q.is_empty() && !name.starts_with(&first_word) && !name.contains(&q) && !alias {
                    continue;
                }
                let line = if q.starts_with(&name) || alias {
                    self.palette_query.trim().to_string()
                } else {
                    name.clone()
                };
                first.get_or_insert(PaletteItem::Command(
                    line.clone(),
                    c.args.iter().any(|a| !a.optional),
                ));
                if ui.selectable(format!("Run: {}  - {}##cmd{name}", c.usage(), c.summary)) {
                    chosen = Some(PaletteItem::Command(line, false));
                }
            }
        }
        if enter {
            chosen = first;
        }
        if ui.is_key_pressed(imgui::Key::Escape) {
            ui.close_current_popup();
            return;
        }
        match chosen {
            Some(PaletteItem::Page(p)) => {
                self.go(p);
                ui.close_current_popup();
            }
            // A bare command that needs arguments: open its form instead of running it.
            Some(PaletteItem::Command(line, needs_args)) if needs_args && !line.contains(' ') => {
                self.go(Page::DebugHelp);
                ui.close_current_popup();
            }
            Some(PaletteItem::Command(line, _)) => {
                crate::devlink::command(&line);
                ui.close_current_popup();
            }
            None => {}
        }
    }
}

enum PaletteItem {
    Page(Page),
    /// The command line to run, and whether it still needs arguments.
    Command(String, bool),
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
    !(admin_only(page) && crate::devlink::registry().1.is_some_and(|m| !m.admin))
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
pub fn table_opts() -> imgui::TableOptions {
    imgui::TableOptions::from(imgui::TableFlags::RESIZABLE | imgui::TableFlags::ROW_BG)
        .sizing_policy(imgui::TableSizingPolicy::FixedFit)
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
