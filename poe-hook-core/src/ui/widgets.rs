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
    if ui.io().key_ctrl()
        && ui.is_key_pressed(imgui::Key::F)
        && ui.is_window_focused_with_flags(imgui::FocusedFlags::ROOT_AND_CHILD_WINDOWS)
    {
        ui.set_keyboard_focus_here();
    }
    input_bar(ui, id, buf, hint, width)
}

/// A toolbar text box that is not a filter (an address, an expression): no `Ctrl+F`.
pub fn input_bar(ui: &Ui, id: &str, buf: &mut String, hint: &str, width: f32) -> bool {
    ui.set_next_item_width(px(ui, width));
    ui.input_text(format!("##{id}"), buf).hint(hint).build()
}

/// Text at the right edge of the current row (the toolbar's counters, "1,204 rows").
pub fn counter(ui: &Ui, text: &str) {
    let w = ui.calc_text_size(text)[0];
    let x = ui.cursor_pos()[0] + ui.content_region_avail()[0] - w;
    ui.same_line();
    // No room left on the row: the counter goes on its own line rather than past the edge.
    if x < ui.cursor_pos()[0] {
        ui.new_line();
    }
    ui.set_cursor_pos([x.max(ui.cursor_pos()[0]), ui.cursor_pos()[1]]);
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

/// A length given for the 13 px base font, at the overlay's current scale.
pub fn px(ui: &Ui, v: f32) -> f32 {
    v * ui.current_font_size() / 13.0
}

/// A muted label before the next widget on the same row ("Cell [x] [y]").
pub fn label(ui: &Ui, text: &str) {
    ui.align_text_to_frame_padding();
    ui.text_disabled(text);
    ui.same_line();
}

/// A button that asks first: `question` in a small popup with `yes` and Cancel (Enter / Escape).
/// Returns true on the frame the user confirmed.
pub fn confirm_button(ui: &Ui, label: &str, question: &str, yes: &str) -> bool {
    let popup = format!("{question}##confirm_{label}");
    if ui.button(label) {
        ui.open_popup(&popup);
    }
    confirm_popup(ui, &popup, question, yes)
}

/// The popup of [`confirm_button`] for a caller that opens it itself (a small button).
pub fn confirm_popup(ui: &Ui, popup: &str, question: &str, yes: &str) -> bool {
    let mut done = false;
    if let Some(_p) = ui.begin_popup(popup) {
        ui.text(question);
        if ui.button(yes) || ui.is_key_pressed(imgui::Key::Enter) {
            done = true;
            ui.close_current_popup();
        }
        ui.same_line();
        if ui.button("Cancel") || ui.is_key_pressed(imgui::Key::Escape) {
            ui.close_current_popup();
        }
    }
    done
}

/// `n thing` / `n things`.
pub fn plural(n: usize, thing: &str) -> String {
    if n == 1 {
        format!("1 {thing}")
    } else if let Some(stem) = thing.strip_suffix('y') {
        format!("{} {stem}ies", thousands(n))
    } else if thing.ends_with("ch") || thing.ends_with('s') {
        format!("{} {thing}es", thousands(n))
    } else {
        format!("{} {thing}s", thousands(n))
    }
}

/// A muted empty-state line ("No packets yet: connect to a server").
pub fn empty(ui: &Ui, text: &str) {
    ui.text_disabled(text);
}

thread_local! {
    /// The splitter being dragged and its width (saved on release).
    static DRAG: std::cell::RefCell<Option<(String, f32)>> = const { std::cell::RefCell::new(None) };
}

/// List on the left, detail on the right, a draggable splitter between them. The left width is
/// kept per `id` in `overlay.toml`.
pub fn split(ui: &Ui, id: &str, default_left: f32, left: impl FnOnce(), right: impl FnOnce()) {
    // Widths are kept for the base font (overlay.toml), so they follow the scale.
    let k = px(ui, 1.0);
    let avail = ui.content_region_avail();
    let (lo, hi) = (120.0 * k, (avail[0] - 160.0 * k).max(120.0 * k));
    let dragging = DRAG.with(|d| {
        d.borrow()
            .as_ref()
            .filter(|(n, _)| n == id)
            .map(|(_, w)| *w)
    });
    let saved = dragging.or_else(|| settings::split_width(id));
    let w = (saved.unwrap_or(default_left) * k).clamp(lo, hi);
    ui.child_window(format!("{id}_left"))
        .size([w, avail[1]])
        .border(true)
        .build(ui, left);
    ui.same_line_with_spacing(0.0, 0.0);
    ui.invisible_button(format!("##{id}_split"), [6.0 * k, avail[1]]);
    if ui.is_item_hovered() || ui.is_item_active() {
        ui.set_mouse_cursor(Some(imgui::MouseCursor::ResizeEW));
    }
    // The width follows the mouse in memory; overlay.toml is written once, on release.
    if ui.is_item_active() {
        let nw = (w + ui.io().mouse_delta()[0]).clamp(lo, hi) / k;
        DRAG.with(|d| *d.borrow_mut() = Some((id.to_string(), nw)));
    }
    if ui.is_item_deactivated()
        && let Some((_, nw)) = DRAG.with(|d| d.borrow_mut().take())
    {
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
    use super::{plural, thousands};

    #[test]
    fn counts_get_the_right_plural() {
        assert_eq!(plural(1, "row"), "1 row");
        assert_eq!(plural(1204, "row"), "1,204 rows");
        assert_eq!(plural(2, "watch"), "2 watches");
        assert_eq!(plural(0, "instance"), "0 instances");
        assert_eq!(plural(3, "entity"), "3 entities");
    }

    #[test]
    fn numbers_get_thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1204), "1,204");
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
