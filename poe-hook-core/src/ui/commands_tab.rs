//! Command forms for the Debug page (our server only): a form for a command of the server's
//! registry (the same commands as `!name` in chat).

use std::collections::HashMap;

use imgui::Ui;

use super::layout_form::LayoutForm;
use crate::devlink::{Cmd, CmdArg};

fn layout_kind(k: &str) -> bool {
    matches!(k, "area" | "seed" | "topology" | "orientation")
}

pub struct CommandsView {
    values: HashMap<String, Vec<String>>,
    forms: HashMap<String, LayoutForm>,
}

impl CommandsView {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            forms: HashMap::new(),
        }
    }

    /// One command's form: its arguments as widgets and a Run button.
    pub fn draw_command(&mut self, ui: &Ui, c: &Cmd, admin: bool, prefix: &str) {
        let _id = ui.push_id(&c.name);
        let values = self
            .values
            .entry(c.name.clone())
            .or_insert_with(|| vec![String::new(); c.args.len()]);
        ui.text(format!("{prefix}{}", c.name));
        if c.admin {
            ui.same_line();
            ui.text_colored(
                if admin {
                    super::theme::WARN
                } else {
                    super::theme::muted(ui)
                },
                "[admin]",
            );
        }
        ui.same_line();
        ui.text_disabled(&c.help);
        let has_layout = c.args.iter().any(|a| a.kind == "area");
        for (i, a) in c.args.iter().enumerate() {
            if has_layout && layout_kind(&a.kind) {
                continue;
            }
            ui.same_line();
            arg_widget(ui, a, &mut values[i]);
        }
        if has_layout {
            let form = self
                .forms
                .entry(c.name.clone())
                .or_insert_with(LayoutForm::new);
            form.draw(ui, &c.name);
        }
        if !has_layout {
            ui.same_line();
        }
        if ui.button(format!("Run##{}", c.name)) {
            let line = build_line(c, &self.values[&c.name], self.forms.get(&c.name));
            crate::devlink::command(&line);
            if let Some(f) = self.forms.get(&c.name) {
                f.used();
            }
        }
        ui.spacing();
    }
}

fn arg_widget(ui: &Ui, a: &CmdArg, value: &mut String) {
    let label = format!(
        "{}{}##{}",
        a.name,
        if a.optional { "?" } else { "" },
        a.name
    );
    match a.kind.as_str() {
        "bool" => {
            let mut on = value == "on";
            if ui.checkbox(&label, &mut on) {
                *value = if on { "on".into() } else { "off".into() };
            }
            if value.is_empty() {
                *value = "off".into();
            }
        }
        k if k.starts_with("choice=") => {
            let choices: Vec<&str> = k["choice=".len()..].split('|').collect();
            let mut i = choices.iter().position(|c| c == value).unwrap_or(0);
            ui.set_next_item_width(120.0);
            if ui.combo_simple_string(&label, &mut i, &choices) || value.is_empty() {
                *value = choices[i].to_string();
            }
        }
        "instance" => {
            let ids: Vec<String> = crate::devlink::instances()
                .map(|(_, l, _)| l.iter().map(|i| format!("{} {}", i.id, i.area)).collect())
                .unwrap_or_default();
            ui.set_next_item_width(90.0);
            ui.input_text(&label, value).build();
            if !ids.is_empty() {
                ui.same_line();
                let mut pick = 0usize;
                let mut items = vec!["pick".to_string()];
                items.extend(ids);
                ui.set_next_item_width(140.0);
                if ui.combo_simple_string(format!("##pick{}", a.name), &mut pick, &items)
                    && pick > 0
                {
                    *value = items[pick].split(' ').next().unwrap_or("").to_string();
                }
            }
        }
        "rest" => {
            ui.set_next_item_width(320.0);
            ui.input_text(&label, value).build();
        }
        _ => {
            ui.set_next_item_width(110.0);
            ui.input_text(&label, value).build();
        }
    }
}

/// The command line from the form: arguments in order; an empty optional argument followed
/// by a filled one gets the kind's "default" word, trailing empty ones are left out.
fn build_line(c: &Cmd, values: &[String], form: Option<&LayoutForm>) -> String {
    let layout: Vec<String> = form
        .map(|f| f.args().split(' ').map(str::to_owned).collect())
        .unwrap_or_default();
    let mut words: Vec<String> = Vec::new();
    let mut li = 0;
    for (i, a) in c.args.iter().enumerate() {
        if form.is_some() && layout_kind(&a.kind) {
            words.push(layout.get(li).cloned().unwrap_or_default());
            li += 1;
            continue;
        }
        words.push(
            values
                .get(i)
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string(),
        );
    }
    while words.last().is_some_and(String::is_empty) {
        words.pop();
    }
    let words: Vec<String> = words
        .into_iter()
        .zip(&c.args)
        .map(|(w, a)| {
            if !w.is_empty() {
                return w;
            }
            match a.kind.as_str() {
                "seed" => "random".into(),
                "topology" | "orientation" => "natural".into(),
                _ => "-".into(),
            }
        })
        .collect();
    std::iter::once(c.name.clone())
        .chain(words)
        .collect::<Vec<_>>()
        .join(" ")
}
