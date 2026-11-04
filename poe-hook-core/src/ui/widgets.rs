//! Shared page furniture: the one-line description with its `(?)` help, status chips, the
//! filter box (`Ctrl+F` focuses it), right-aligned counters and the draggable splitter.

use imgui::Ui;

use super::{settings, theme};

/// Colour class of a [`chip`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Ok,
    Warn,
    Bad,
    Muted,
}

/// A small coloured text tag: `[PRIVATE]`, `[sync ok]`. Stays on the line (use `same_line`).
pub fn chip(ui: &Ui, text: &str, tone: Tone) {
    let color = match tone {
        Tone::Ok => theme::OK,
        Tone::Warn => theme::WARN,
        Tone::Bad => theme::BAD,
        Tone::Muted => theme::muted(ui),
    };
    ui.text_colored(color, format!("[{text}]"));
}

/// The line under a page's tab bar: a muted description and a `(?)` that shows `help` on hover.
pub fn header(ui: &Ui, description: &str, help: &str) {
    ui.text_disabled(description);
    if !help.is_empty() {
        ui.same_line();
        ui.text_disabled("(?)");
        if ui.is_item_hovered() {
            ui.tooltip_text(help);
        }
    }
}

/// The toolbar's filter box: first on the row, `Ctrl+F` focuses it. Returns true when it changed.
pub fn filter(ui: &Ui, id: &str, buf: &mut String, hint: &str, width: f32) -> bool {
    if ui.io().key_ctrl() && ui.is_key_pressed(imgui::Key::F) {
        ui.set_keyboard_focus_here();
    }
    ui.set_next_item_width(width);
    ui.input_text(format!("##{id}"), buf).hint(hint).build()
}

/// Text at the right edge of the current row (the toolbar's counters, "1,204 rows").
pub fn counter(ui: &Ui, text: &str) {
    let w = ui.calc_text_size(text)[0];
    let x = ui.cursor_pos()[0] + ui.content_region_avail()[0] - w;
    ui.same_line();
    if x > ui.cursor_pos()[0] {
        ui.set_cursor_pos([x, ui.cursor_pos()[1]]);
    }
    ui.text_disabled(text);
}

/// `1204` as `1,204`.
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A muted empty-state line ("No packets yet: connect to a server").
pub fn empty(ui: &Ui, text: &str) {
    ui.text_disabled(text);
}

/// List on the left, detail on the right, a draggable splitter between them. The left width is
/// kept per `id` in `overlay.toml`.
pub fn split(ui: &Ui, id: &str, default_left: f32, left: impl FnOnce(), right: impl FnOnce()) {
    let avail = ui.content_region_avail();
    let saved = settings::get().split_widths.get(id).copied();
    let w = saved
        .unwrap_or(default_left)
        .clamp(120.0, (avail[0] - 160.0).max(120.0));
    ui.child_window(format!("{id}_left"))
        .size([w, avail[1]])
        .border(true)
        .build(ui, left);
    ui.same_line_with_spacing(0.0, 0.0);
    ui.invisible_button(format!("##{id}_split"), [6.0, avail[1]]);
    if ui.is_item_hovered() || ui.is_item_active() {
        ui.set_mouse_cursor(Some(imgui::MouseCursor::ResizeEW));
    }
    if ui.is_item_active() {
        let nw = (w + ui.io().mouse_delta()[0]).clamp(120.0, (avail[0] - 160.0).max(120.0));
        settings::update(|s| {
            s.split_widths.insert(id.to_string(), nw);
        });
    }
    ui.same_line_with_spacing(0.0, 0.0);
    ui.child_window(format!("{id}_right"))
        .size([0.0, avail[1]])
        .border(true)
        .build(ui, right);
}

#[cfg(test)]
mod tests {
    use super::thousands;

    #[test]
    fn numbers_get_thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1204), "1,204");
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
