//! Player, Entities and Watches tabs: live, read-only views of game memory.

use imgui::{ListClipper, Ui};

use super::widgets::{self, Tone, split};

use crate::watches::{KINDS, Kind, Watch};

pub fn draw_player(ui: &Ui) {
    widgets::header(
        ui,
        "The local player's entity, live.",
        "Found when the player entity is created or ticks. Cells = world / (250/23).",
    );
    let Some(p) = crate::inspect::player() else {
        widgets::empty(ui, "No player entity yet: enter an area.");
        return;
    };
    ui.child_window("overview")
        .border(true)
        .size([0.0, 0.0])
        .build(ui, || {
            ui.text(format!("Entity {:#x}   id {}", p.entity, p.id));
            ui.separator();
            ui.text(format!("Cell  {}, {}", p.grid.0, p.grid.1));
            ui.text(format!(
                "World {:.2}, {:.2}   = cells {:.2}, {:.2}",
                p.world.0,
                p.world.1,
                p.world.0 / (250.0 / 23.0),
                p.world.1 / (250.0 / 23.0)
            ));
            ui.text(format!(
                "Facing {:.3} rad ({:.1} deg)",
                p.facing,
                p.facing.to_degrees()
            ));
            if p.facing_locked {
                ui.same_line();
                widgets::chip(ui, "locked", Tone::Warn);
            }
            ui.separator();
            ui.text(format!("Actions {:#06x} / {:#06x}", p.action, p.action2));
            ui.text(format!("WASD direction ({}, {})", p.wasd.0, p.wasd.1));
            ui.text(format!("Actor flags +0x2a2 = {:#04x}", p.flags));
            if p.flags & 0x20 != 0 {
                ui.same_line();
                widgets::chip(ui, "sprint held", Tone::Ok);
            }
        });
}

pub struct EntitiesView {
    filter: String,
    alive_only: bool,
    selected: Option<usize>,
}

impl EntitiesView {
    pub fn new() -> Self {
        Self {
            filter: String::new(),
            alive_only: true,
            selected: None,
        }
    }

    pub fn draw(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "Entities created in this area (from the initial-state reader).",
            "Pick one to see its components; scan sends a component to the Scanner.",
        );
        widgets::filter(ui, "ent_filter", &mut self.filter, "filter path/id", 260.0);
        ui.same_line();
        ui.checkbox("alive", &mut self.alive_only);
        let f = self.filter.to_lowercase();
        let all = crate::entities::list();
        let total = all.len();
        let rows: Vec<_> = all
            .into_iter()
            .map(|e| {
                let live = crate::entities::live(&e);
                (e, live)
            })
            .filter(|(e, l)| {
                (!self.alive_only || l.alive)
                    && (f.is_empty()
                        || e.path.to_lowercase().contains(&f)
                        || format!("{:#x}", e.id).contains(&f)
                        || e.id.to_string() == f)
            })
            .collect();
        widgets::counter(
            ui,
            &format!(
                "{} of {}",
                widgets::thousands(rows.len()),
                widgets::plural(total, "entity")
            ),
        );
        if rows.is_empty() {
            widgets::empty(
                ui,
                if total == 0 {
                    "No entities yet: enter an area."
                } else {
                    "No entity matches the filter (or none is alive)."
                },
            );
            return;
        }
        let selected = std::cell::Cell::new(self.selected);
        split(
            ui,
            "entities",
            ui.content_region_avail()[0] * 0.55,
            || {
                if let Some(_t) = crate::ui::list_table(
                    ui,
                    "entity_list2",
                    ["id", "type", "grid"].map(imgui::TableColumnSetup::new),
                    "type",
                ) {
                    let mut clipper = ListClipper::new(rows.len()).begin(ui);
                    while clipper.step() {
                        for i in clipper.display_start()..clipper.display_end() {
                            let (e, l) = &rows[i];
                            ui.table_next_column();
                            if ui
                                .selectable_config(format!("{:#x}##ent{}", e.id, e.ptr))
                                .selected(selected.get() == Some(e.ptr))
                                .span_all_columns(true)
                                .build()
                            {
                                selected.set(Some(e.ptr));
                            }
                            ui.table_next_column();
                            let name = if e.path.is_empty() {
                                format!("key {:#010x}", e.type_key)
                            } else {
                                e.path.clone()
                            };
                            if l.alive {
                                ui.text(&name);
                            } else {
                                ui.text_disabled(format!("{name} (gone)"));
                            }
                            if ui.is_item_hovered() {
                                ui.tooltip_text(&name);
                            }
                            ui.table_next_column();
                            ui.text(l.grid.map_or("-".into(), |g| format!("{}, {}", g.0, g.1)));
                        }
                    }
                }
            },
            || match rows.iter().find(|(e, _)| Some(e.ptr) == selected.get()) {
                Some((e, l)) => {
                    ui.text(if e.path.is_empty() {
                        format!("key {:#010x}", e.type_key)
                    } else {
                        e.path.clone()
                    });
                    ui.text_disabled(format!(
                        "id {:#x}   address {:#x}   grid {}",
                        e.id,
                        e.ptr,
                        l.grid.map_or("-".into(), |g| format!("{}, {}", g.0, g.1))
                    ));
                    if let Some(p) = crate::inspect::player()
                        && let Some(g) = l.grid
                    {
                        let d = ((g.0 - p.grid.0) as f32).hypot((g.1 - p.grid.1) as f32);
                        ui.text_disabled(format!("distance {d:.0} cells"));
                    }
                    ui.separator();
                    components(ui, e.ptr);
                }
                None => widgets::empty(ui, "Pick an entity on the left."),
            },
        );
        self.selected = selected.get();
    }
}

/// The selected entity's components: name, address, vtable; "scan" sends one to the Scanner.
fn components(ui: &Ui, ptr: usize) {
    let names = crate::entities::component_names(ptr);
    let comps = crate::entities::components(ptr);
    ui.text(format!("Components of {ptr:#x} ({}):", comps.len()));
    let main = crate::dump_types::main_module();
    if let Some(_t) = ui.begin_table_header_with_flags(
        "components",
        [
            imgui::TableColumnSetup::new("#"),
            imgui::TableColumnSetup::new("name"),
            imgui::TableColumnSetup::new("address"),
            imgui::TableColumnSetup::new("vtable"),
            imgui::TableColumnSetup::new(""),
        ],
        crate::ui::table_opts(),
    ) {
        for (i, c) in comps.iter().enumerate() {
            let name = names
                .iter()
                .find(|(n, _)| *n == i)
                .map_or("?", |(_, s)| s.as_str());
            let vt = poe_hook_shared::mem::read::<usize>(*c).unwrap_or(0);
            ui.table_next_column();
            ui.text(i.to_string());
            ui.table_next_column();
            ui.text(name);
            ui.table_next_column();
            ui.text(format!("{c:#x}"));
            ui.table_next_column();
            ui.text(match main.filter(|m| m.contains(vt)) {
                Some(m) => format!("{:#x}", m.to_ghidra(vt)),
                None => format!("{vt:#x}"),
            });
            ui.table_next_column();
            if ui.small_button(format!("scan##c{i}")) {
                crate::ui::scanner_request(format!("{c:#x}"), 0x600);
            }
        }
    }
}

pub struct WatchesView {
    list: Vec<Watch>,
    new_expr: String,
    new_kind: usize,
}

impl WatchesView {
    pub fn new() -> Self {
        Self {
            list: crate::watches::load(),
            new_expr: String::new(),
            new_kind: 2,
        }
    }

    pub fn draw(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "Read-only memory watches.",
            "Syntax: numbers (0x.. hex; 0x14.. = Ghidra address), + and -, [expr] reads a \
             pointer; names: player, pos, actor, exe. Example: pos+0x44c (f32 facing).",
        );
        widgets::input_bar(ui, "watch_expr", &mut self.new_expr, "expression", 360.0);
        let enter = ui.is_item_deactivated() && ui.is_key_pressed(imgui::Key::Enter);
        ui.same_line();
        ui.set_next_item_width(crate::ui::widgets::px(ui, 90.0));
        let names: Vec<&str> = KINDS.iter().map(|k| k.name()).collect();
        ui.combo_simple_string("##kind", &mut self.new_kind, &names);
        ui.same_line();
        if (ui.button("Add") || enter) && !self.new_expr.trim().is_empty() {
            self.list.push(Watch {
                expr: self.new_expr.trim().to_owned(),
                kind: KINDS[self.new_kind],
            });
            crate::watches::save(&self.list);
        }
        widgets::counter(ui, &widgets::plural(self.list.len(), "watch"));
        if self.list.is_empty() {
            widgets::empty(ui, "No watches yet: add an expression above.");
            return;
        }
        let mut remove = None;
        if let Some(_t) = crate::ui::list_table(
            ui,
            "watches",
            ["", "type", "expression", "value"].map(imgui::TableColumnSetup::new),
            "value",
        ) {
            for (i, w) in self.list.iter().enumerate() {
                ui.table_next_column();
                if ui.small_button(format!("x##w{i}")) {
                    remove = Some(i);
                }
                ui.table_next_column();
                ui.text(w.kind.name());
                ui.table_next_column();
                ui.text(&w.expr);
                ui.table_next_column();
                ui.text(crate::watches::value(w));
            }
        }
        if let Some(i) = remove {
            self.list.remove(i);
            crate::watches::save(&self.list);
        }
        let _ = Kind::U8;
    }
}
