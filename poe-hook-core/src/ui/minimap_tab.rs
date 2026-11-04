//! Minimap tab: the client's cell grids around the player (shade = the 4-bit value), the
//! player and its facing, and the server's idea of the player (our server). Which grid is
//! walkability is not settled: the player was seen standing on grid 0 = 0 with grids 1-3 = 5. "Export" writes the area's
//! layout and grids (`terrain_layout_*.tsv`, `terrain_grid_*.bin`) for validating our terrain
//! generator (`poe-world`), also on official servers.

use imgui::Ui;

use super::widgets;
use crate::minimap::Grid;

pub struct MinimapView {
    layer: usize,
    cell_px: f32,
    follow: bool,
    show_server: bool,
    /// The centre cell while not following.
    anchor: (i64, i64),
}

impl MinimapView {
    pub fn new() -> Self {
        Self {
            layer: 1,
            cell_px: 4.0,
            follow: true,
            show_server: true,
            anchor: (0, 0),
        }
    }

    pub fn draw(&mut self, ui: &Ui) {
        widgets::header(
            ui,
            "The client's cell grids around the player.",
            "Shade = the grid's 4-bit value. Which grid is walkability is not settled: the player \
             was seen standing on grid 0 = 0 with grids 1-3 = 5. Export writes the area's layout \
             and grids for validating our terrain generator, also on official servers.",
        );
        ui.set_next_item_width(70.0);
        ui.combo_simple_string("grid", &mut self.layer, &["0", "1", "2", "3"]);
        ui.same_line();
        ui.set_next_item_width(140.0);
        ui.slider("zoom", 1.0, 10.0, &mut self.cell_px);
        ui.same_line();
        if ui.checkbox("follow player", &mut self.follow)
            && !self.follow
            && let Some(p) = crate::inspect::player()
        {
            self.anchor = (p.grid.0 as i64, p.grid.1 as i64);
        }
        ui.same_line();
        ui.checkbox("server position", &mut self.show_server);
        ui.same_line();
        if ui.button("Export area") {
            let ok = crate::dump_types::export_layout_now();
            crate::ui::notice(match crate::terrain_tools::export_now() {
                Some(dir) => format!(
                    "Area exported to {} (with server entities and walkability probe)",
                    dir.display()
                ),
                None if ok => {
                    "Area layout and grids exported (dump_dir terrain/layouts and terrain/grids)"
                        .into()
                }
                None => "No area loaded".to_string(),
            });
        }
        let Some(grid) = Grid::current(self.layer) else {
            widgets::empty(ui, "No terrain: enter an area.");
            return;
        };
        let Some(p) = crate::inspect::player() else {
            widgets::empty(ui, "No player yet.");
            return;
        };
        let (px, py) = if self.follow {
            (p.grid.0 as i64, p.grid.1 as i64)
        } else {
            self.anchor
        };
        let here: Vec<String> = (0..4)
            .map(|l| {
                Grid::current(l)
                    .and_then(|g| g.get(p.grid.0 as i64, p.grid.1 as i64))
                    .map_or("-".into(), |v| v.to_string())
            })
            .collect();
        widgets::counter(
            ui,
            &format!(
                "area {}x{}  player ({}, {})  grids here: {}",
                grid.cells_x,
                grid.cells_y,
                p.grid.0,
                p.grid.1,
                here.join(" ")
            ),
        );
        let cell = self.cell_px;
        let avail = ui.content_region_avail();
        let side = avail[0].min(avail[1]).max(cell * 3.0);
        let r = (((side / cell) as i64 - 1) / 2).max(1);
        let origin = ui.cursor_screen_pos();
        let size = (2 * r + 1) as f32 * cell;
        let draw = ui.get_window_draw_list();
        draw.add_rect(
            origin,
            [origin[0] + size, origin[1] + size],
            [0.4, 0.4, 0.4, 1.0],
        )
        .build();
        // Rows top (high y) to bottom: the game's y grows "up" on screen in this view.
        for dy in -r..=r {
            let y = py + dy;
            let sy = origin[1] + (r - dy) as f32 * cell;
            let mut run_start = -r;
            let mut run_val = grid.get(px - r, y);
            for dx in -r..=r + 1 {
                let v = if dx <= r {
                    grid.get(px + dx, y)
                } else {
                    Some(255)
                };
                if v != run_val {
                    if let Some(val) = run_val.filter(|&v| v != 0) {
                        let s = 0.2 + val as f32 / 15.0 * 0.7;
                        let shade = [0.15, s, 0.25, 1.0];
                        let x0 = origin[0] + (run_start + r) as f32 * cell;
                        let x1 = origin[0] + (dx + r) as f32 * cell;
                        draw.add_rect([x0, sy], [x1, sy + cell], shade)
                            .filled(true)
                            .build();
                    }
                    run_start = dx;
                    run_val = v;
                }
            }
        }
        // The player and its facing (angle 0 = +y; atan2(dy, dx) + pi/2).
        let at = |gx: i64, gy: i64| {
            [
                origin[0] + ((gx - px + r) as f32 + 0.5) * cell,
                origin[1] + ((r - (gy - py)) as f32 + 0.5) * cell,
            ]
        };
        let c = at(p.grid.0 as i64, p.grid.1 as i64);
        draw.add_circle(c, (cell * 1.5).max(3.0), [1.0, 0.9, 0.2, 1.0])
            .filled(true)
            .build();
        let a = p.facing - std::f32::consts::FRAC_PI_2;
        let len = (cell * 6.0).max(14.0);
        draw.add_line(
            c,
            [c[0] + a.cos() * len, c[1] - a.sin() * len],
            [1.0, 0.9, 0.2, 1.0],
        )
        .thickness(2.0)
        .build();
        if self.show_server
            && let Some(s) = crate::devlink::server_cell()
        {
            let sc = at(s.0 as i64, s.1 as i64);
            draw.add_circle(sc, (cell * 1.5).max(3.0), [0.3, 0.7, 1.0, 1.0])
                .thickness(2.0)
                .build();
        }
        ui.dummy([size, size]);
        // Hover: the cell under the mouse and its four grid values.
        if ui.is_item_hovered() {
            let m = ui.io().mouse_pos();
            let gx = px + ((m[0] - origin[0]) / cell).floor() as i64 - r;
            let gy = py + r - ((m[1] - origin[1]) / cell).floor() as i64;
            let vals: Vec<String> = (0..4)
                .map(|l| {
                    Grid::current(l)
                        .and_then(|g| g.get(gx, gy))
                        .map_or("-".into(), |v| v.to_string())
                })
                .collect();
            ui.tooltip_text(format!("cell {gx}, {gy}\ngrids 0-3: {}", vals.join(" ")));
        }
        // Legend in the corner.
        let lx = origin[0] + 8.0;
        let ly = origin[1] + 8.0;
        draw.add_text(
            [lx, ly],
            [1.0, 0.9, 0.2, 1.0],
            "yellow: client position, facing",
        );
        draw.add_text(
            [lx, ly + 16.0],
            [0.3, 0.7, 1.0, 1.0],
            "blue ring: server position",
        );
        draw.add_text(
            [lx, ly + 32.0],
            [0.5, 0.9, 0.5, 1.0],
            "green shade: grid value",
        );
    }
}
