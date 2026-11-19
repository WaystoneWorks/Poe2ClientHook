//! Snap detector (`[features] snap_log`, on by default; read-only): logs every server position
//! the client applies to the local player and whether it snapped the player.
//!
//! In lockstep the client snaps the player to the cell a message carries exactly when that cell
//! differs from its grid (Positioned `+0x444/+0x448`) at the frame the message is applied
//! (docs/re/server-code.md, section 1, in the main repository):
//! - `0x64` MoveDirectionUpdate with a non-zero vector, in the Actor handler `0x142159130`
//!   (`rcx` Actor, `rdx` message: opcode `+0x8`, vector `+0x20`, cell `+0x28/+0x2c`);
//! - `0x1c8` StartAction, when `Actor_InstallQueuedMove` `0x141e50030` installs the queued
//!   action (`rcx` Actor, `rdx` slot; start `actor+0x3bc/+0x3c0`, `INT_MIN` for none).
//!
//! Lines (all for the local player, `frame` = the player tick the apply precedes, the same
//! numbering as the movement log's `frame #N` / `LINE frame=N` / `INPUT frame=N`):
//! - `SNAPCHK frame=N op=0x64|0x1c8 cell=(x,y) grid=(x,y) world=(x,y) ...` for every apply
//!   (a zero-vector `0x64` as `SNAPCHK frame=N op=0x64 cell=(x,y) vec=(0,0) nocheck`);
//! - `SNAP frame=N op=.. cell=(x,y) grid=(x,y) d=<cells> jump=<world units>` when it snaps;
//! - `FORCED frame=N caller=<ghidra> grid=(x,y) -> cell=(x,y) world=(x,y)` for every call of
//!   the snap setter `Positioned_SetLocationForcedNoCallbacks` `0x141de8e70` on the player
//!   (also warps and other callers), with the caller's return address.
//!
//! `scripts/re/snaps.py <session>` lists a session's snaps.

use ilhook::x64::{HookType, Registers};
use poe_hook_shared::mem::read;

const OP_MOVE_DIRECTION: u16 = 0x64;
const OP_START_ACTION: u16 = 0x1c8;
/// World units per grid cell (`10.869565`, the client's cell-to-world factor).
const CELL: f32 = 250.0 / 23.0;
/// Message offsets in the Actor handler (`0x142159207..0x142159254`).
const MSG_OPCODE: usize = 0x8;
const MSG_VECTOR: usize = 0x20;
const MSG_CELL: usize = 0x28;
/// Actor fields read by `Actor_InstallQueuedMove`: the owner entity, the StartAction start and
/// the queued action (type at `+0x120`).
const ACTOR_ENTITY: usize = 0x8;
const ACTOR_START: usize = 0x3bc;
const ACTOR_QUEUED: usize = 0x248;
const ACTION_TYPE: usize = 0x120;

pub fn init() -> Result<(), String> {
    use crate::sigs::addr;
    let handler = addr("actor_sim_handler")?;
    let install = addr("install_queued_move")?;
    let setter = addr("set_location_forced_nc")?;
    crate::hooks::install(handler, HookType::JmpBack(on_actor_message), 0)
        .map_err(|e| format!("hook actor handler: {e}"))?;
    crate::hooks::install(install, HookType::JmpBack(on_install_queued_move), 0)
        .map_err(|e| format!("hook install queued move: {e}"))?;
    crate::hooks::install(setter, HookType::JmpBack(on_set_location_forced), 0)
        .map_err(|e| format!("hook set location forced: {e}"))
}

/// The local player's Positioned component, if `entity` is the player.
pub(crate) fn player_positioned(entity: usize) -> Option<usize> {
    let player = crate::dump_types::player();
    if player == 0 || entity != player {
        return None;
    }
    let comps = read::<usize>(player + crate::sigs::off().entity_components_begin)?;
    read::<usize>(comps).filter(|&p| p > 0x10000)
}

pub(crate) fn grid(pos: usize) -> (i32, i32) {
    let o = crate::sigs::off();
    (
        read::<i32>(pos + o.positioned_grid_x).unwrap_or(i32::MIN),
        read::<i32>(pos + o.positioned_grid_y).unwrap_or(i32::MIN),
    )
}

fn world(pos: usize) -> (f32, f32) {
    let o = crate::sigs::off();
    (
        read::<f32>(pos + o.positioned_world_x).unwrap_or(f32::NAN),
        read::<f32>(pos + o.positioned_world_y).unwrap_or(f32::NAN),
    )
}

/// The player tick this apply precedes (messages are applied in the frame drain, before the
/// entities' ticks).
pub(crate) fn frame() -> usize {
    crate::dump_types::player_updates() + 1
}

/// Logs one apply and, when the cell differs from the grid, the snap it causes.
fn check(op: u16, pos: usize, cell: (i32, i32), extra: &str) {
    let g = grid(pos);
    let w = world(pos);
    let n = frame();
    log::info!(
        "SNAPCHK frame={n} op={op:#x} cell=({},{}) grid=({},{}) world=({:.3},{:.3}){extra}",
        cell.0,
        cell.1,
        g.0,
        g.1,
        w.0,
        w.1
    );
    if cell != g {
        let (dx, dy) = ((cell.0 - g.0) as f32, (cell.1 - g.1) as f32);
        let (tx, ty) = ((cell.0 as f32 + 0.5) * CELL, (cell.1 as f32 + 0.5) * CELL);
        log::info!(
            "SNAP frame={n} op={op:#x} cell=({},{}) grid=({},{}) d={:.2} jump={:.2}",
            cell.0,
            cell.1,
            g.0,
            g.1,
            (dx * dx + dy * dy).sqrt(),
            ((tx - w.0).powi(2) + (ty - w.1).powi(2)).sqrt()
        );
    }
}

unsafe extern "win64" fn on_actor_message(regs: *mut Registers, _: usize) {
    let _guard = crate::hook_guard!("snap_log::on_actor_message");
    let r = unsafe { &*regs };
    let (actor, msg) = (r.rcx as usize, r.rdx as usize);
    if read::<u16>(msg + MSG_OPCODE) != Some(OP_MOVE_DIRECTION) {
        return;
    }
    let Some(pos) = read::<usize>(actor + ACTOR_ENTITY).and_then(player_positioned) else {
        return;
    };
    let v = (
        read::<i32>(msg + MSG_VECTOR).unwrap_or(0),
        read::<i32>(msg + MSG_VECTOR + 4).unwrap_or(0),
    );
    let (Some(x), Some(y)) = (read::<i32>(msg + MSG_CELL), read::<i32>(msg + MSG_CELL + 4)) else {
        return;
    };
    // A zero vector never checks its cell (0x142159220..0x14215922d): logged without a check,
    // so that a teleport (a non-zero `0x64` then a zero one at the same cell) can be told apart.
    if v == (0, 0) {
        log::info!(
            "SNAPCHK frame={} op=0x64 cell=({x},{y}) vec=(0,0) nocheck",
            frame()
        );
        return;
    }
    check(
        OP_MOVE_DIRECTION,
        pos,
        (x, y),
        &format!(" vec=({},{})", v.0, v.1),
    );
}

unsafe extern "win64" fn on_install_queued_move(regs: *mut Registers, _: usize) {
    let _guard = crate::hook_guard!("snap_log::on_install_queued_move");
    let r = unsafe { &*regs };
    let (actor, slot) = (r.rcx as usize, r.rdx as usize);
    // An occupied slot returns before the check (0x141e50038).
    if read::<usize>(slot) != Some(0) {
        return;
    }
    let Some(pos) = read::<usize>(actor + ACTOR_ENTITY).and_then(player_positioned) else {
        return;
    };
    let (Some(x), Some(y)) = (
        read::<i32>(actor + ACTOR_START),
        read::<i32>(actor + ACTOR_START + 4),
    ) else {
        return;
    };
    if x == i32::MIN {
        return;
    }
    let ty = read::<usize>(actor + ACTOR_QUEUED)
        .filter(|&a| a > 0x10000)
        .and_then(|a| read::<u16>(a + ACTION_TYPE))
        .unwrap_or(0);
    check(OP_START_ACTION, pos, (x, y), &format!(" type={ty:#x}"));
}

unsafe extern "win64" fn on_set_location_forced(regs: *mut Registers, _: usize) {
    let _guard = crate::hook_guard!("snap_log::on_set_location_forced");
    let r = unsafe { &*regs };
    let pos = r.rcx as usize;
    if player_positioned(crate::dump_types::player()) != Some(pos) {
        return;
    }
    let cell = r.rdx as usize;
    let (x, y) = (
        read::<i32>(cell).unwrap_or(i32::MIN),
        read::<i32>(cell + 4).unwrap_or(i32::MIN),
    );
    let w = r.r8 as usize;
    let (wx, wy) = (
        read::<f32>(w).unwrap_or(f32::NAN),
        read::<f32>(w + 4).unwrap_or(f32::NAN),
    );
    let g = grid(pos);
    let ret = read::<usize>(r.rsp as usize).unwrap_or(0);
    log::info!(
        "FORCED frame={} caller={:#x} grid=({},{}) -> cell=({x},{y}) world=({wx:.3},{wy:.3})",
        frame(),
        crate::sigs::ghidra(ret),
        g.0,
        g.1
    );
}
