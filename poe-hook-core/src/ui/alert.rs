//! `ui::alert`: a centred window for things that need reading (the terrain mismatch popup).
//! Alerts queue; the first one is drawn until the player presses OK. They are drawn without the
//! main window and take mouse and keyboard while they are shown (`overlay::input_active`).

use std::{collections::VecDeque, path::PathBuf, sync::Mutex};

use imgui::{Condition, Ui, WindowFlags};

use super::theme;

/// One `label  value  [status]` line of an alert.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub label: String,
    pub value: String,
    /// A word after the value with its colour (`DIFFERENT`, `same`).
    pub status: Option<(String, [f32; 4])>,
}

impl Row {
    pub fn new(label: &str, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            status: None,
        }
    }

    pub fn with_status(mut self, text: &str, colour: [f32; 4]) -> Self {
        self.status = Some((text.into(), colour));
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub title: String,
    pub rows: Vec<Row>,
    /// Text of the "Copy report" button (no button when `None`).
    pub report: Option<String>,
    /// Folder of the "Open folder" button (no button when `None`).
    pub folder: Option<PathBuf>,
}

static QUEUE: Mutex<VecDeque<Alert>> = Mutex::new(VecDeque::new());

/// Queues an alert (also logged).
pub fn alert(a: Alert) {
    log::warn!("alert: {}", a.title);
    QUEUE.lock().unwrap().push_back(a);
}

pub fn has_alerts() -> bool {
    !QUEUE.lock().unwrap().is_empty()
}

/// The alert being shown, if any.
pub fn current() -> Option<Alert> {
    QUEUE.lock().unwrap().front().cloned()
}

fn dismiss() {
    QUEUE.lock().unwrap().pop_front();
}

/// Draws the first queued alert.
pub fn draw(ui: &Ui) {
    let Some(alert) = current() else { return };
    let [w, h] = ui.io().display_size();
    let mut done = false;
    // Centred on the screen (the window class has no pivot setter).
    unsafe {
        imgui::sys::igSetNextWindowPos(
            imgui::sys::ImVec2 {
                x: w * 0.5,
                y: h * 0.4,
            },
            Condition::Always as i32,
            imgui::sys::ImVec2 { x: 0.5, y: 0.5 },
        );
    }
    ui.window(format!("{}##alert", alert.title))
        .flags(
            WindowFlags::ALWAYS_AUTO_RESIZE
                | WindowFlags::NO_COLLAPSE
                | WindowFlags::NO_SAVED_SETTINGS
                | WindowFlags::NO_MOVE,
        )
        .build(|| {
            for row in &alert.rows {
                ui.text_colored(theme::WARN, format!("{:<8}", row.label));
                ui.same_line();
                ui.text(&row.value);
                if let Some((text, colour)) = &row.status {
                    ui.same_line();
                    ui.text_colored(*colour, text);
                }
            }
            ui.separator();
            if let Some(report) = &alert.report
                && ui.button("Copy report")
            {
                super::set_clipboard(report);
            }
            if let Some(folder) = &alert.folder {
                ui.same_line();
                if ui.button("Open folder") {
                    // Best effort (under Wine `explorer` may do nothing).
                    let _ = std::process::Command::new("explorer").arg(folder).spawn();
                }
            }
            ui.same_line();
            if ui.button("OK") {
                done = true;
            }
        });
    if done {
        dismiss();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alerts_queue_and_dismiss_in_order() {
        QUEUE.lock().unwrap().clear();
        let make = |t: &str| Alert {
            title: t.into(),
            rows: vec![Row::new("Area", "G1_2").with_status("same", theme::OK)],
            report: None,
            folder: None,
        };
        alert(make("one"));
        alert(make("two"));
        assert!(has_alerts());
        assert_eq!(current().unwrap().title, "one");
        dismiss();
        assert_eq!(current().unwrap().title, "two");
        dismiss();
        assert!(!has_alerts());
    }
}
