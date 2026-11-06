//! Command forms for the Debug page (our server only): a form for a command of the server's
//! registry (the same commands as `!name` in chat), generated from its arguments and flags.

use std::collections::HashMap;

use imgui::Ui;
use poe_hook_shared::cmdspec::form_line;

use super::{layout_form::LayoutForm, widgets};
use crate::devlink::{Cmd, CmdArg};

fn layout_kind(k: &str) -> bool {
    matches!(k, "area" | "seed" | "topology" | "orientation")
}

/// What a form holds for one command.
#[derive(Default)]
struct Values {
    args: Vec<String>,
    /// `--flag` values; a switch is `on` when set.
    flags: Vec<String>,
}

pub struct CommandsView {
    values: HashMap<String, Values>,
    forms: HashMap<String, LayoutForm>,
}

impl CommandsView {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            forms: HashMap::new(),
        }
    }

    /// Every command of a Debug tab that has no hand-laid form.
    pub fn draw_page(&mut self, ui: &Ui, page: &str, cmds: &[Cmd], skip: &[&str], prefix: &str) {
        for c in cmds
            .iter()
            .filter(|c| c.overlay.as_deref() == Some(page))
            .filter(|c| !skip.contains(&c.name().as_str()))
        {
            self.draw_command(ui, c, prefix);
            ui.separator();
        }
    }

    /// One command's form: its arguments and flags as widgets and a Run button.
    pub fn draw_command(&mut self, ui: &Ui, c: &Cmd, prefix: &str) {
        let name = c.name();
        let _id = ui.push_id(&name);
        ui.text(format!("{prefix}{name}"));
        ui.same_line();
        ui.text_disabled(&c.summary);
        let values = self.values.entry(name.clone()).or_insert_with(|| Values {
            args: vec![String::new(); c.args.len()],
            flags: vec![String::new(); c.flags.len()],
        });
        let has_layout = c.args.iter().any(|a| a.kind == "area");
        let mut enter = false;
        let mut first = true;
        let mut place = |ui: &Ui| {
            if !first {
                ui.same_line();
            }
            first = false;
        };
        for (i, a) in c.args.iter().enumerate() {
            if has_layout && layout_kind(&a.kind) {
                continue;
            }
            place(ui);
            enter |= arg_widget(ui, a, &mut values.args[i]);
        }
        for (i, f) in c.flags.iter().enumerate() {
            place(ui);
            match &f.kind {
                Some(kind) => {
                    let a = CmdArg {
                        name: format!("--{}", f.name),
                        kind: kind.clone(),
                        optional: true,
                    };
                    enter |= arg_widget(ui, &a, &mut values.flags[i]);
                }
                None => {
                    let mut on = values.flags[i] == "on";
                    if ui.checkbox(format!("--{}", f.name), &mut on) {
                        values.flags[i] = if on { "on".into() } else { String::new() };
                    }
                }
            }
        }
        if has_layout {
            let form = self
                .forms
                .entry(name.clone())
                .or_insert_with(LayoutForm::new);
            form.draw(ui, &name);
        } else {
            place(ui);
        }
        // Every required argument filled in (the layout form's area counts for its kinds).
        let missing: Vec<&str> = c
            .args
            .iter()
            .enumerate()
            .filter(|(_, a)| !a.optional)
            .filter(|(i, a)| {
                if has_layout && layout_kind(&a.kind) {
                    a.kind == "area" && !self.forms.get(&name).is_some_and(|f| f.has_area())
                } else {
                    self.values[&name].args[*i].trim().is_empty()
                }
            })
            .map(|(_, a)| a.name.as_str())
            .collect();
        let ready = missing.is_empty();
        let run = {
            let _d = ui.begin_disabled_with_cond(!ready);
            ui.button(format!("Run##{name}"))
        };
        if !ready && ui.is_item_hovered_with_flags(imgui::ItemHoveredFlags::ALLOW_WHEN_DISABLED) {
            ui.tooltip_text(format!("Fill in: {}", missing.join(", ")));
        }
        if (run || enter) && ready {
            let line = build_line(c, &self.values[&name], self.forms.get(&name));
            crate::devlink::command(&line);
            if let Some(f) = self.forms.get(&name) {
                f.used();
            }
        }
        ui.spacing();
    }
}

/// One argument's widget. Text fields show the argument's name as their hint (a trailing `?`
/// marks an optional one); choices and pickers have a muted label in front. Returns true when
/// Enter was pressed in a text field.
fn arg_widget(ui: &Ui, a: &CmdArg, value: &mut String) -> bool {
    let em = ui.current_font_size();
    let name = format!("{}{}", a.name, if a.optional { "?" } else { "" });
    let id = format!("##{}", a.name);
    let text = |ui: &Ui, value: &mut String, width: f32| {
        ui.set_next_item_width(width * em);
        let enter = ui
            .input_text(&id, value)
            .hint(&name)
            .enter_returns_true(true)
            .build();
        if ui.is_item_hovered() {
            ui.tooltip_text(format!("{name} ({})", a.kind));
        }
        enter
    };
    let picker = |ui: &Ui, value: &mut String, items: Vec<String>, take_first_word: bool| {
        if items.is_empty() {
            return;
        }
        ui.same_line();
        let mut pick = 0usize;
        let mut list = vec!["pick...".to_string()];
        list.extend(items);
        ui.set_next_item_width(9.0 * em);
        if ui.combo_simple_string(format!("##pick{}", a.name), &mut pick, &list) && pick > 0 {
            *value = if take_first_word {
                list[pick].split(' ').next().unwrap_or("").to_string()
            } else {
                list[pick].clone()
            };
        }
    };
    match a.kind.as_str() {
        "bool" => {
            let mut on = value == "on";
            if ui.checkbox(format!("{name}{id}"), &mut on) {
                *value = if on { "on".into() } else { "off".into() };
            }
            if value.is_empty() {
                *value = "off".into();
            }
            false
        }
        k if k.starts_with("choice=") => {
            let choices: Vec<&str> = k["choice=".len()..].split('|').collect();
            let mut i = choices.iter().position(|c| c == value).unwrap_or(0);
            widgets::label(ui, &name);
            ui.set_next_item_width(8.0 * em);
            if ui.combo_simple_string(&id, &mut i, &choices) || value.is_empty() {
                *value = choices[i].to_string();
            }
            false
        }
        "instance" => {
            let enter = text(ui, value, 6.0);
            let ids: Vec<String> = crate::devlink::instances()
                .map(|(_, l, _)| l.iter().map(|i| format!("{} {}", i.id, i.area)).collect())
                .unwrap_or_default();
            picker(ui, value, ids, true);
            enter
        }
        "player" => {
            let enter = text(ui, value, 8.0);
            let names: Vec<String> = crate::devlink::online()
                .into_iter()
                .map(|o| o.character)
                .collect();
            picker(ui, value, names, false);
            enter
        }
        "rest" => text(ui, value, 24.0),
        _ => text(ui, value, 8.0),
    }
}

/// The command line from the form.
fn build_line(c: &Cmd, values: &Values, form: Option<&LayoutForm>) -> String {
    let layout: Vec<String> = form
        .map(|f| f.args().split(' ').map(str::to_owned).collect())
        .unwrap_or_default();
    let mut args = Vec::with_capacity(c.args.len());
    let mut li = 0;
    for (i, a) in c.args.iter().enumerate() {
        if form.is_some() && layout_kind(&a.kind) {
            args.push(layout.get(li).cloned().unwrap_or_default());
            li += 1;
        } else {
            args.push(values.args.get(i).cloned().unwrap_or_default());
        }
    }
    form_line(c, &args, &values.flags)
}
