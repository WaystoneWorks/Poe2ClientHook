//! "Follow the newest line" for the live lists (Log, Packets, Console).
//!
//! The view follows while it is at the bottom: scrolling up pauses it, scrolling (or jumping)
//! back to the bottom follows again, and a small "Jump to latest" button shows while paused.
//! The old rule ("follow if within 40 px of the bottom after the new lines were added") lost
//! the bottom as soon as more than about two lines arrived in one frame.
//!
//! Use inside the scrolling child: [`Follow::begin`] before the lines, [`Follow::end`] after.

use imgui::Ui;

/// How close to the bottom (px) counts as "at the bottom".
const SLACK: f32 = 2.0;

pub struct Follow {
    on: bool,
    /// Lines there were when following stopped (for the "N new" count).
    paused_at: usize,
    /// Paused with the checkbox: stays paused at the bottom too.
    held: bool,
    /// [`Follow::jump`] was called before this frame's [`Follow::begin`]: follow without looking.
    jumping: bool,
}

/// Whether to follow this frame, from the scroll position at the start of the frame. ImGui's
/// scroll and its maximum both describe the last frame's content and the scroll requested at
/// its end, so while following the view sits exactly at the bottom however many lines arrived;
/// it is above the bottom only when the user scrolled up (pause) and back at it when the user
/// scrolled down again or pressed "Jump to latest" (follow).
fn next(scroll_y: f32, scroll_max_y: f32) -> bool {
    scroll_y >= scroll_max_y - SLACK
}

/// A scroll target past any content: ImGui clamps it to the bottom when it applies it.
const BOTTOM: f32 = 1.0e7;

impl Follow {
    pub fn new() -> Self {
        Self {
            on: true,
            paused_at: 0,
            held: false,
            jumping: false,
        }
    }

    pub fn following(&self) -> bool {
        self.on
    }

    /// Starts following again (scrolls to the bottom at the end of this frame).
    pub fn jump(&mut self) {
        self.on = true;
        self.held = false;
        self.jumping = true;
    }

    /// Stops following until [`Follow::jump`] (the Follow checkbox): unlike a scroll up, reaching
    /// the bottom does not resume it.
    pub fn pause(&mut self, lines: usize) {
        self.on = false;
        self.held = true;
        self.paused_at = lines;
    }

    /// Call first inside the scrolling child: reads where the user left the view.
    pub fn begin(&mut self, ui: &Ui, lines: usize) {
        let was = self.on;
        if std::mem::take(&mut self.jumping) {
            self.on = true;
        } else if self.on || !self.held {
            self.on = next(ui.scroll_y(), ui.scroll_max_y());
        }
        if was && !self.on {
            self.paused_at = lines;
        }
    }

    /// Call last inside a scrolling table: scrolls to the bottom while following; draws nothing
    /// (the caller shows [`Follow::status`] on its toolbar instead of the corner button).
    pub fn end_quiet(&mut self, ui: &Ui) {
        if self.on {
            ui.set_scroll_y(BOTTOM);
        }
    }

    /// "Paused, 12 new" while paused, for a toolbar.
    pub fn status(&self, lines: usize) -> Option<String> {
        if self.on {
            return None;
        }
        let new = lines.saturating_sub(self.paused_at);
        Some(if new > 0 {
            format!("Paused, {new} new")
        } else {
            "Paused".to_string()
        })
    }

    /// Call last inside the scrolling child: scrolls to the bottom while following, or draws the
    /// "Jump to latest" button in the child's bottom-right corner while paused.
    pub fn end(&mut self, ui: &Ui, lines: usize) {
        if self.on {
            ui.set_scroll_y(BOTTOM);
            return;
        }
        let new = lines.saturating_sub(self.paused_at);
        let label = if new > 0 {
            format!("Paused, {new} new  -  Jump to latest")
        } else {
            "Paused  -  Jump to latest".to_string()
        };
        let [w, h] = ui.window_size();
        let k = super::widgets::px(ui, 1.0);
        let tw = ui.calc_text_size(&label)[0] + 24.0 * k;
        let bh = ui.frame_height();
        // In content coordinates: the visible bottom-right corner (above the scrollbars).
        ui.set_cursor_pos([
            ui.scroll_x() + (w - tw - 22.0 * k).max(0.0),
            ui.scroll_y() + (h - bh - 18.0 * k).max(0.0),
        ]);
        if ui.button_with_size(&label, [tw, bh]) {
            self.held = false;
            self.on = true;
            ui.set_scroll_y(BOTTOM);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::next;

    #[test]
    fn following_stops_when_scrolled_up() {
        assert!(!next(400.0, 900.0));
    }

    #[test]
    fn following_survives_many_new_lines() {
        // At the bottom of the last frame's content, however many lines arrive now.
        assert!(next(900.0, 900.0));
    }

    #[test]
    fn scrolling_back_down_follows_again() {
        assert!(!next(880.0, 900.0));
        assert!(next(899.0, 900.0));
    }

    #[test]
    fn short_content_follows() {
        assert!(next(0.0, 0.0));
    }
}
