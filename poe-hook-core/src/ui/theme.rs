//! The overlay's look: Dear ImGui's stock dark style (colours, spacing, rounding) and the
//! built-in font, scaled by `overlay.scale`. Only the semantic colours ImGui has no slot for
//! live here: use these instead of literal RGBA.

use imgui::{StyleColor, Ui};

/// Good / on / allowed.
pub const OK: [f32; 4] = [0.40, 0.85, 0.40, 1.0];
/// Needs attention: warnings, changed values, the read-only badge.
pub const WARN: [f32; 4] = [1.00, 0.85, 0.25, 1.0];
/// Errors, failures, slow things.
pub const BAD: [f32; 4] = [1.00, 0.40, 0.35, 1.0];

/// Secondary text: ImGui's `TextDisabled`.
pub fn muted(ui: &Ui) -> [f32; 4] {
    ui.style_color(StyleColor::TextDisabled)
}

/// Resets the context's style to ImGui's stock dark one. Call it before `scale_all_sizes`, so
/// the sizes scale with the overlay.
pub fn apply(style: &mut imgui::Style) {
    imgui::Theme {
        preset: imgui::ThemePreset::Dark,
        ..imgui::Theme::default()
    }
    .apply_to_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stock_dark_style_is_applied() {
        let mut ctx = imgui::Context::create();
        let style = ctx.style_mut();
        style.set_color(StyleColor::WindowBg, [1.0; 4]);
        apply(style);
        assert_eq!(style.color(StyleColor::WindowBg)[0], 0.06);
    }
}
