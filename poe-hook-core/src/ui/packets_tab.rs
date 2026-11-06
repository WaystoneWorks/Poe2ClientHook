//! Packets tab: every captured message (login and instance, both directions) with filters,
//! and the selected message decoded (known layouts) and as a hex dump. The same messages are in
//! `packets.jsonl` / `packets.txt` in the session folder.

use std::sync::Arc;

use imgui::{ListClipper, Ui};

use super::widgets;

use crate::packets::{Channel, Dir, Msg};

/// Opcodes hidden by "hide noise": frame reports, pings and per-tick messages.
fn noisy(m: &Msg) -> bool {
    matches!(
        m.name(),
        Some("FrameReport" | "PingReply" | "SimulationTick" | "KeepAlive" | "KeepAliveReply")
    )
}

pub struct PacketsView {
    msgs: Vec<Arc<Msg>>,
    last_seq: u64,
    filter: String,
    show_in: bool,
    show_out: bool,
    channels: [bool; 3],
    hide_noise: bool,
    paused: bool,
    follow: super::follow::Follow,
    selected: Option<u64>,
    shown: Vec<usize>,
    shown_key: (String, bool, bool, [bool; 3], bool, u64, bool),
    /// Show opcodes hidden in `hidden_opcodes.tsv` anyway.
    show_hidden: bool,
    shown_upto: usize,
    detail: Option<(u64, String)>,
    /// Note editors for the selected message's opcode and for the message itself.
    opcode_note: String,
    message_note: String,
}

impl PacketsView {
    pub fn new() -> Self {
        Self {
            msgs: Vec::new(),
            last_seq: 0,
            filter: String::new(),
            show_in: true,
            show_out: true,
            channels: [true; 3],
            hide_noise: true,
            paused: false,
            follow: super::follow::Follow::new(),
            selected: None,
            shown: Vec::new(),
            shown_key: (String::new(), true, true, [true; 3], true, 0, false),
            show_hidden: false,
            shown_upto: 0,
            detail: None,
            opcode_note: String::new(),
            message_note: String::new(),
        }
    }

    fn pull(&mut self) {
        if self.paused {
            return;
        }
        let new = crate::packets::since(self.last_seq);
        if let Some(m) = new.last() {
            self.last_seq = m.seq;
        }
        self.msgs.extend(new);
        let keep = crate::config().packets.keep.max(100);
        if self.msgs.len() > keep {
            let drop = self.msgs.len() - keep;
            self.msgs.drain(..drop);
            self.shown.clear();
            self.shown_upto = 0;
        }
    }

    fn passes(&self, m: &Msg) -> bool {
        let dir_ok = match m.dir {
            Dir::In => self.show_in,
            Dir::Out => self.show_out,
        };
        let chan_ok = self.channels[match m.channel {
            Channel::Login => 0,
            Channel::Instance => 1,
            Channel::Other => 2,
        }];
        if !dir_ok || !chan_ok || (self.hide_noise && noisy(m)) {
            return false;
        }
        if !self.show_hidden
            && crate::hidden::is_hidden(crate::hidden::key(m.channel, m.dir, m.opcode))
        {
            return false;
        }
        if self.filter.is_empty() {
            return true;
        }
        let f = self.filter.to_lowercase();
        format!("{:#06x}", m.opcode).contains(&f)
            || m.name().is_some_and(|n| n.to_lowercase().contains(&f))
    }

    fn refresh(&mut self) {
        let key = (
            self.filter.clone(),
            self.show_in,
            self.show_out,
            self.channels,
            self.hide_noise,
            crate::hidden::version(),
            self.show_hidden,
        );
        if key != self.shown_key {
            self.shown_key = key;
            self.shown.clear();
            self.shown_upto = 0;
        }
        for i in self.shown_upto..self.msgs.len() {
            if self.passes(&self.msgs[i]) {
                self.shown.push(i);
            }
        }
        self.shown_upto = self.msgs.len();
    }

    pub fn draw(&mut self, ui: &Ui) {
        self.pull();
        widgets::header(
            ui,
            "Every captured message, login and instance, both directions.",
            "All messages are also in packets.jsonl / packets.txt in the session folder. The \
             Opcodes tab hides or shows whole opcodes.",
        );
        widgets::filter(ui, "pk_filter", &mut self.filter, "filter name/0x..", 200.0);
        ui.same_line();
        // Direction and channel filters in a popup, so the toolbar fits a narrow window.
        let narrowed =
            !(self.show_in && self.show_out && self.channels.iter().all(|c| *c)) || self.hide_noise;
        if ui.button(if narrowed {
            "Show: some v"
        } else {
            "Show: all v"
        }) {
            ui.open_popup("##pk_show");
        }
        if let Some(_p) = ui.begin_popup("##pk_show") {
            ui.text_disabled("direction");
            ui.checkbox("in (server to client)", &mut self.show_in);
            ui.checkbox("out (client to server)", &mut self.show_out);
            ui.separator();
            ui.text_disabled("connection");
            for (i, name) in ["login", "instance", "other"].iter().enumerate() {
                ui.checkbox(name, &mut self.channels[i]);
            }
            ui.separator();
            ui.checkbox("hide noise (per-frame messages)", &mut self.hide_noise);
        }
        ui.same_line();
        ui.checkbox("pause", &mut self.paused);
        ui.same_line();
        let mut following = self.follow.following();
        if ui.checkbox("follow", &mut following) {
            if following {
                self.follow.jump();
            } else {
                self.follow.pause(self.shown.len());
            }
        }
        ui.same_line();
        ui.checkbox(
            format!("show hidden ({})", crate::hidden::count()),
            &mut self.show_hidden,
        );
        self.refresh();
        widgets::counter(
            ui,
            &format!(
                "{} of {}{}",
                widgets::thousands(self.shown.len()),
                widgets::thousands(self.msgs.len()),
                if crate::config().packets.redact {
                    ", secrets redacted"
                } else {
                    ""
                }
            ),
        );
        if self.msgs.is_empty() {
            widgets::empty(ui, "No packets yet: connect to a server.");
            return;
        }
        let selected = std::cell::Cell::new(self.selected);
        let mut follow = std::mem::replace(&mut self.follow, super::follow::Follow::new());
        let (msgs, shown) = (&self.msgs, &self.shown);
        let msg = self
            .selected
            .and_then(|seq| self.msgs.iter().find(|m| m.seq == seq).cloned());
        if let Some(m) = &msg
            && self.detail.as_ref().is_none_or(|(s, _)| *s != m.seq)
        {
            self.opcode_note = crate::notes::opcode_note(m.channel, m.dir, m.opcode);
            self.message_note = crate::notes::message_note(m);
            self.detail = Some((m.seq, detail_text(m)));
        }
        let text = self
            .detail
            .as_ref()
            .map(|(_, t)| t.clone())
            .unwrap_or_default();
        let (opcode_note, message_note) = (&mut self.opcode_note, &mut self.message_note);
        widgets::split(
            ui,
            "packets",
            620.0,
            || {
                let flags = imgui::TableFlags::RESIZABLE
                    | imgui::TableFlags::ROW_BG
                    | imgui::TableFlags::BORDERS_INNER_V
                    | imgui::TableFlags::SCROLL_Y;
                if let Some(_t) = crate::ui::list_table(
                    ui,
                    "packet_list",
                    ["seq", "time", "dir", "op", "name", "size"].map(imgui::TableColumnSetup::new),
                    "name",
                ) {
                    follow.begin(ui, shown.len());
                    let mut clipper = ListClipper::new(shown.len()).begin(ui);
                    while clipper.step() {
                        for row in clipper.display_start()..clipper.display_end() {
                            let m = &msgs[shown[row as usize]];
                            let color = match m.dir {
                                Dir::In => super::theme::OK,
                                Dir::Out => super::theme::WARN,
                            };
                            ui.table_next_column();
                            if ui
                                .selectable_config(format!("{}##{}", m.seq, m.seq))
                                .selected(selected.get() == Some(m.seq))
                                .span_all_columns(true)
                                .build()
                            {
                                selected.set(Some(m.seq));
                                follow.pause(shown.len());
                            }
                            ui.table_next_column();
                            ui.text(crate::util::clock(m.unix_ms));
                            ui.table_next_column();
                            ui.text_colored(color, m.dir.name());
                            ui.table_next_column();
                            ui.text(format!("{:#06x}", m.opcode));
                            ui.table_next_column();
                            // Notes: [note] on this message, * on its opcode.
                            let mark = match (
                                crate::notes::has_message_note(m),
                                crate::notes::has_opcode_note(m.channel, m.dir, m.opcode),
                            ) {
                                (true, _) => "  [note]",
                                (false, true) => "  *",
                                _ => "",
                            };
                            ui.text(format!("{}{mark}", m.name().unwrap_or("?")));
                            ui.table_next_column();
                            ui.text(format!(
                                "{}{}",
                                m.len,
                                if m.framed { "" } else { " unframed" }
                            ));
                        }
                    }
                    follow.end(ui, shown.len());
                }
            },
            || {
                let Some(m) = &msg else {
                    widgets::empty(ui, "Select a message to see it decoded.");
                    return;
                };
                let name = m.name().unwrap_or("?");
                if ui.button("Copy") {
                    crate::ui::set_clipboard(&text);
                }
                ui.same_line();
                let k = crate::hidden::key(m.channel, m.dir, m.opcode);
                let hidden = crate::hidden::is_hidden(k);
                if ui.button(if hidden {
                    "Show this opcode"
                } else {
                    "Hide this opcode"
                }) {
                    crate::hidden::set_hidden(k, !hidden);
                    crate::ui::notice(format!(
                        "{:#06x} {name} {}",
                        m.opcode,
                        if hidden {
                            "shown again"
                        } else {
                            "hidden (\"show hidden\" or the Opcodes tab brings it back)"
                        }
                    ));
                }
                ui.text_disabled(format!(
                    "Note on every {} {} {:#06x} {name}:",
                    m.channel.name(),
                    m.dir.name(),
                    m.opcode
                ));
                ui.input_text_multiline(
                    "##opcode_note",
                    opcode_note,
                    [-1.0, crate::ui::widgets::px(ui, 48.0)],
                )
                .build();
                if ui.button("Save opcode note") {
                    crate::notes::set_opcode_note(m.channel, m.dir, m.opcode, opcode_note);
                    crate::ui::notice(format!("Note saved for {:#06x} {name}", m.opcode));
                }
                ui.text_disabled(format!("Note on this message only (#{}):", m.seq));
                ui.input_text_multiline(
                    "##message_note",
                    message_note,
                    [-1.0, crate::ui::widgets::px(ui, 48.0)],
                )
                .build();
                if ui.button("Save message note") {
                    crate::notes::set_message_note(m, message_note);
                    crate::ui::notice(format!("Note saved for message #{}", m.seq));
                }
                ui.separator();
                ui.child_window("packet_detail")
                    .flags(imgui::WindowFlags::HORIZONTAL_SCROLLBAR)
                    .build(ui, || {
                        ui.text(&text);
                    });
            },
        );
        self.selected = selected.get();
        self.follow = follow;
    }
}

/// The selected message as text: header, decoded fields, hex dump.
fn detail_text(m: &Msg) -> String {
    let mut text = format!(
        "#{} {} {} {} {:#06x} {} ({} bytes{}{})\n\n",
        m.seq,
        crate::util::clock(m.unix_ms),
        m.dir.name(),
        m.channel.name(),
        m.opcode,
        m.name().unwrap_or("unknown opcode"),
        m.len,
        if m.framed { "" } else { ", unframed" },
        if m.official { ", official" } else { "" }
    );
    match crate::packets::describe(m) {
        Some(d) => text.push_str(&d),
        None => text.push_str("(no decoder for this message)"),
    }
    text.push_str("\n\n");
    text.push_str(&crate::util::hex_lines(&m.data));
    text
}
