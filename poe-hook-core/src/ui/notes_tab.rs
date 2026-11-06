//! Notes tab: every packet note (opcode notes first), as saved in
//! `logs/poe-hook/packet_notes.tsv`. Notes are written in the Packets and Protocol tabs.

use imgui::Ui;

use super::widgets;

pub struct NotesView {
    filter: String,
}

impl NotesView {
    pub fn new() -> Self {
        Self {
            filter: String::new(),
        }
    }

    pub fn draw(&mut self, ui: &Ui) {
        draw(ui, &mut self.filter);
    }
}

fn draw(ui: &Ui, filter: &mut String) {
    widgets::header(
        ui,
        "Every packet note, opcode notes first.",
        "Saved in logs/poe-hook/packet_notes.tsv (kept across sessions). Write them in the \
         Messages or Protocol page.",
    );
    widgets::filter(ui, "notes_filter", filter, "filter notes", 260.0);
    let f = filter.to_lowercase();
    let notes: Vec<_> = crate::notes::all()
        .into_iter()
        .filter(|n| {
            f.is_empty()
                || n.text.to_lowercase().contains(&f)
                || format!("{:#06x}", n.opcode).contains(&f)
                || crate::packets::name(n.channel, n.dir, n.opcode)
                    .is_some_and(|x| x.to_lowercase().contains(&f))
        })
        .collect();
    widgets::counter(ui, &widgets::plural(notes.len(), "note"));
    if notes.is_empty() {
        widgets::empty(ui, "No notes yet: write one on the Messages tab.");
        return;
    }
    if let Some(_t) = crate::ui::list_table(
        ui,
        "notes",
        [
            imgui::TableColumnSetup::new("on"),
            imgui::TableColumnSetup::new("channel"),
            imgui::TableColumnSetup::new("dir"),
            imgui::TableColumnSetup::new("opcode"),
            imgui::TableColumnSetup::new("name"),
            imgui::TableColumnSetup::new("when"),
            imgui::TableColumnSetup::new("note"),
        ],
        "note",
    ) {
        for n in &notes {
            ui.table_next_column();
            ui.text(if n.message {
                format!("message #{}", n.seq)
            } else {
                "opcode".into()
            });
            ui.table_next_column();
            ui.text(n.channel.name());
            ui.table_next_column();
            ui.text(n.dir.name());
            ui.table_next_column();
            ui.text(format!("{:#06x}", n.opcode));
            ui.table_next_column();
            ui.text(crate::packets::name(n.channel, n.dir, n.opcode).unwrap_or("?"));
            ui.table_next_column();
            ui.text(if n.message {
                format!("{} {}", n.session, crate::util::clock(n.unix_ms))
            } else {
                crate::util::clock(n.unix_ms)
            });
            ui.table_next_column();
            ui.text_wrapped(&n.text);
        }
    }
}
