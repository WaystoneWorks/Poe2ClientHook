//! Coverage tab: every opcode seen, per channel and direction, with counts on official and on
//! our servers (all sessions, `logs/poe-hook/coverage.tsv`) and this session. "Official only"
//! lists what official servers send that ours never did: the to-do list for what the server still has to implement.

use imgui::Ui;

use super::widgets;

use crate::packets::{Channel, Dir};

pub struct CoverageView {
    official_only: bool,
    unnamed_only: bool,
    filter: String,
}

impl CoverageView {
    pub fn new() -> Self {
        Self {
            official_only: false,
            unnamed_only: false,
            filter: String::new(),
        }
    }

    pub fn draw(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "Every opcode seen, with counts on official and our servers, and its visibility.",
            "All sessions are merged in logs/poe-hook/coverage.tsv. \"official only\" lists what \
             official servers send that ours never did: the server's to-do list. The visible \
             column hides the opcode in the Messages tab.",
        );
        widgets::filter(
            ui,
            "cov_filter",
            &mut self.filter,
            "filter name/0x..",
            220.0,
        );
        ui.same_line();
        ui.checkbox("official only", &mut self.official_only);
        ui.same_line();
        ui.checkbox("no name yet", &mut self.unnamed_only);
        let f = self.filter.to_lowercase();
        let rows: Vec<_> = crate::packets::coverage()
            .into_iter()
            .filter(|((c, d, op), s)| {
                let name = crate::packets::name(*c, *d, *op);
                (!self.official_only || (s.official > 0 && s.private == 0))
                    && (!self.unnamed_only || name.is_none())
                    && (f.is_empty()
                        || format!("{op:#06x} {}", name.unwrap_or(""))
                            .to_lowercase()
                            .contains(&f))
            })
            .collect();
        ui.same_line();
        if ui.button("Hide listed") {
            for ((c, d, op), _) in &rows {
                crate::hidden::set_hidden(crate::hidden::key(*c, *d, *op), true);
            }
        }
        ui.same_line();
        if ui.button("Show all") {
            crate::hidden::clear();
        }
        widgets::counter(ui, &format!("{} opcodes", widgets::thousands(rows.len())));
        if rows.is_empty() {
            widgets::empty(ui, "No opcodes yet: connect to a server.");
            return;
        }
        if let Some(_t) = crate::ui::list_table(
            ui,
            "coverage2",
            [
                "visible", "channel", "dir", "opcode", "name", "official", "ours", "session",
                "size",
            ]
            .map(imgui::TableColumnSetup::new),
            "name",
        ) {
            for ((c, d, op), s) in rows {
                let name = crate::packets::name(c, d, op);
                let key = crate::hidden::key(c, d, op);
                ui.table_next_column();
                let mut visible = !crate::hidden::is_hidden(key);
                if ui.checkbox(format!("##vis{}_{}_{op}", c.name(), d.name()), &mut visible) {
                    crate::hidden::set_hidden(key, !visible);
                }
                ui.table_next_column();
                ui.text(c.name());
                ui.table_next_column();
                ui.text(d.name());
                if ui.is_item_hovered() {
                    ui.tooltip_text(match d {
                        Dir::In => "server to client",
                        Dir::Out => "client to server",
                    });
                }
                ui.table_next_column();
                ui.text(format!("{op:#06x}"));
                ui.table_next_column();
                match name {
                    Some(n) => ui.text(n),
                    None => ui.text_colored(super::theme::WARN, "?"),
                }
                ui.table_next_column();
                ui.text(s.official.to_string());
                ui.table_next_column();
                if s.private == 0 && s.official > 0 {
                    ui.text_colored(super::theme::WARN, "0");
                } else {
                    ui.text(s.private.to_string());
                }
                ui.table_next_column();
                ui.text(s.session.to_string());
                ui.table_next_column();
                ui.text(if s.min_len == s.max_len {
                    format!("{}", s.min_len)
                } else {
                    format!("{}..{}", s.min_len, s.max_len)
                });
            }
        }
        let _ = Channel::Login;
    }
}
