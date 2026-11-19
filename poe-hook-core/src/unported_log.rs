//! Unported-code flags (`[features] unported_log`, on by default; read-only): logs when the
//! local player's movement runs client code the server's copy (`clientsim`/`clientpath`) has
//! not ported. The list and the per-session hits are in docs/re/movement-coverage.md (main
//! repository).
//!
//! One line per site and player frame (the first call of that frame; the same numbering as the
//! movement log's `frame #N` and the snap detector's lines):
//! `UNPORTED frame=N fn=<name> caller=<ghidra> grid=(x,y) ...` with
//! - `astar_fallback`: the general A* `0x141edc7d0` for the player (the search `0x141edeb10`
//!   calls it only for a start inside an entity or a start too narrow for the mover, and for
//!   its coarse retry); `goal=(x,y) coarse=0|1`;
//! - `push_test`: the push test `0x141dee9e0` with the player as the mover;
//! - `can_push`: `0x141df03c0` with the player as the mover (`other=(x,y)` the other's grid);
//! - `step_entity_hit`: the step check's entity collision `0x141dfdb80` blocked the player's
//!   step (`Positioned_CheckStep` out = 3, `0x141deadc1`); `hit=(x,y)`;
//! - `move_event_clipped`: the clipped branch of the move event end `0x141f70e30` (event
//!   `+0xa8` set and its clip cell `+0xa0` differs from the end cell `+0x98`);
//!   `end=(x,y) clip=(x,y)`.

use std::sync::atomic::{AtomicUsize, Ordering};

use ilhook::x64::{HookType, JmpBackRoutine, Registers};
use poe_hook_shared::mem::read;

use crate::snap_log::{frame, grid, player_positioned};

const ASTAR: usize = 0;
const PUSH_TEST: usize = 1;
const CAN_PUSH: usize = 2;
const STEP_HIT: usize = 3;
const MOVE_CLIPPED: usize = 4;
const NAMES: [&str; 5] = [
    "astar_fallback",
    "push_test",
    "can_push",
    "step_entity_hit",
    "move_event_clipped",
];
/// The last player frame each site logged (one line per site and frame).
static LAST: [AtomicUsize; 5] = [const { AtomicUsize::new(usize::MAX) }; 5];

/// `0x141f70e30` event fields: end cell, clip cell, clip flag, the mover's Positioned holder.
const EV_END: usize = 0x98;
const EV_CLIP: usize = 0xa0;
const EV_CLIPPED: usize = 0xa8;
const EV_SKIP: usize = 0xac;
const EV_OWNER: usize = 0xb0;
const OWNER_POSITIONED: usize = 0x98;
/// `Positioned_CheckStep`'s frame slot holding the hit Positioned (`0x141deadc7`).
const STEP_HIT_SLOT: usize = 0xe60;

pub fn init() -> Result<(), String> {
    use crate::sigs::addr;
    let hooks: [(&str, JmpBackRoutine); 5] = [
        ("astar_general", on_astar),
        ("push_test", on_push_test),
        ("can_push", on_can_push),
        ("step_entity_hit", on_step_hit),
        ("move_event_end", on_move_event_end),
    ];
    for (name, cb) in hooks {
        crate::hooks::install(addr(name)?, HookType::JmpBack(cb), 0)
            .map_err(|e| format!("hook {name}: {e}"))?;
    }
    Ok(())
}

fn cell(p: usize) -> (i32, i32) {
    (
        read::<i32>(p).unwrap_or(i32::MIN),
        read::<i32>(p + 4).unwrap_or(i32::MIN),
    )
}

/// Logs a hit of `site` once per player frame.
fn hit(site: usize, pos: usize, ret: usize, extra: impl FnOnce() -> String) {
    let n = frame();
    if LAST[site].swap(n, Ordering::Relaxed) == n {
        return;
    }
    let g = grid(pos);
    log::info!(
        "UNPORTED frame={n} fn={} caller={:#x} grid=({},{}){}",
        NAMES[site],
        crate::sigs::ghidra(ret),
        g.0,
        g.1,
        extra()
    );
}

fn ret_addr(r: &Registers) -> usize {
    read::<usize>(r.rsp as usize).unwrap_or(0)
}

/// The local player's Positioned, if `pos` is it.
fn is_player_positioned(pos: usize) -> bool {
    pos != 0 && player_positioned(crate::dump_types::player()) == Some(pos)
}

unsafe extern "win64" fn on_astar(regs: *mut Registers, _: usize) {
    let _guard = crate::hook_guard!("unported_log::on_astar");
    let r = unsafe { &*regs };
    let Some(pos) = player_positioned(r.rdx as usize) else {
        return;
    };
    let goal = cell(r.rcx as usize);
    // param_11 (coarse) is the 11th argument: [rsp + 0x58] at entry.
    let coarse = read::<u8>(r.rsp as usize + 0x58).unwrap_or(0);
    hit(ASTAR, pos, ret_addr(r), || {
        format!(" goal=({},{}) coarse={coarse}", goal.0, goal.1)
    });
}

unsafe extern "win64" fn on_push_test(regs: *mut Registers, _: usize) {
    let _guard = crate::hook_guard!("unported_log::on_push_test");
    let r = unsafe { &*regs };
    let pos = r.rcx as usize;
    if is_player_positioned(pos) {
        hit(PUSH_TEST, pos, ret_addr(r), String::new);
    }
}

unsafe extern "win64" fn on_can_push(regs: *mut Registers, _: usize) {
    let _guard = crate::hook_guard!("unported_log::on_can_push");
    let r = unsafe { &*regs };
    let pos = r.rcx as usize;
    if is_player_positioned(pos) {
        let other = grid(r.rdx as usize);
        hit(CAN_PUSH, pos, ret_addr(r), || {
            format!(" other=({},{})", other.0, other.1)
        });
    }
}

unsafe extern "win64" fn on_step_hit(regs: *mut Registers, _: usize) {
    let _guard = crate::hook_guard!("unported_log::on_step_hit");
    let r = unsafe { &*regs };
    // Mid-function site: rdi = P for the whole of Positioned_CheckStep.
    let pos = r.rdi as usize;
    if is_player_positioned(pos) {
        let other = read::<usize>(r.rbp as usize + STEP_HIT_SLOT).unwrap_or(0);
        let h = grid(other);
        // The caller of CheckStep is not on top of the stack here; log the site itself.
        hit(
            STEP_HIT,
            pos,
            crate::sigs::addr("step_entity_hit").unwrap_or(0),
            || format!(" hit=({},{})", h.0, h.1),
        );
    }
}

unsafe extern "win64" fn on_move_event_end(regs: *mut Registers, _: usize) {
    let _guard = crate::hook_guard!("unported_log::on_move_event_end");
    let r = unsafe { &*regs };
    let ev = r.rcx as usize;
    if read::<i32>(ev + EV_SKIP) != Some(0) || read::<u8>(ev + EV_CLIPPED).unwrap_or(0) == 0 {
        return;
    }
    let (end, clip) = (cell(ev + EV_END), cell(ev + EV_CLIP));
    if end == clip {
        return;
    }
    let Some(pos) = read::<usize>(ev + EV_OWNER).and_then(|o| read::<usize>(o + OWNER_POSITIONED))
    else {
        return;
    };
    if is_player_positioned(pos) {
        hit(MOVE_CLIPPED, pos, ret_addr(r), || {
            format!(" end=({},{}) clip=({},{})", end.0, end.1, clip.0, clip.1)
        });
    }
}
