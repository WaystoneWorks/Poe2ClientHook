//! Research aid (`POE_HOOK_PHASE_LOG=1`): where in the player's frame the sprint's end and the
//! keyed messages run. Read-only, logs at function entry.
//!
//! - `SCHK frame comp phase`: `SprintPart_EndCheck` `0x141b76da0`.
//! - `SEND frame comp phase bonk no_anim ra`: `SprintPart_End` `0x141b773a0`.
//! - `DRN frame comp phase key=comp,seq vt`: packet apply `0x1421343c0` for a packet keyed to the
//!   player. `vt` is the packet's vtable (Ghidra address).
//! - `PH frame comp phase ra`: phase step `0x1421303e0`, before the increment.
//! - `TOL frame stat ra 142 2b4 id`: the facing tolerance `0x14197e160` (vf `+0x348`) of a player
//!   action: `stat` = `action_required_target_facing_angle_tolerance_degrees` (0x46a7) when it is
//!   read and non-zero (hook `0x14197e1c6`), `2pi` when the function returns 2*pi (hook
//!   `0x14197e1e3`); `ra` = caller.
//! - `FACE frame comp phase new old ra`: `Positioned_SetFacing` `0x141ddc820` of the player, at its
//!   store `0x141ddc91a` (`xmm0` = new angle before the wrap, `+0x44c` = old; locked calls do not
//!   get there). A hook at the function's entry hangs the client at startup. When `ra` is
//!   `0x141e50a5c` (actor set-target `0x141e50940`), its own caller follows.
//!
//! Frame state: world `+0x678` entity key, `+0x67c` component, `+0x67e` phase; packet header `+0x14`
//! entity key, `+0x18` component, `+0x1a` seq; entity `+0x78` world, `+0x88` key
//! (docs/re/wasd-movement.md, "Frame drain").

use std::sync::OnceLock;

use ilhook::x64::{HookType, Registers};
use poe_hook_shared::mem::read;
use poe_hook_shared::module::Module;

static MAIN: OnceLock<Module> = OnceLock::new();

const SPRINT_CHECK: usize = 0x141b76da0;
const SPRINT_END: usize = 0x141b773a0;
const PACKET_APPLY: usize = 0x1421343c0;
const PHASE_STEP: usize = 0x1421303e0;
const TOL_STAT: usize = 0x14197e1c6;
const TOL_2PI: usize = 0x14197e1e3;
const SET_FACING: usize = 0x141ddc91a;

pub fn init(main: &Module) -> Result<(), String> {
    MAIN.set(*main).ok();
    for (at, f) in [
        (SPRINT_CHECK, on_check as ilhook::x64::JmpBackRoutine),
        (SPRINT_END, on_end as ilhook::x64::JmpBackRoutine),
        (PACKET_APPLY, on_packet as ilhook::x64::JmpBackRoutine),
        (PHASE_STEP, on_phase as ilhook::x64::JmpBackRoutine),
        (TOL_STAT, on_tol_stat as ilhook::x64::JmpBackRoutine),
        (TOL_2PI, on_tol_2pi as ilhook::x64::JmpBackRoutine),
        (SET_FACING, on_set_facing as ilhook::x64::JmpBackRoutine),
    ] {
        let runtime = main.from_ghidra(at);
        crate::hooks::install(runtime, HookType::JmpBack(f), 0)
            .map_err(|e| format!("hook {at:#x}: {e}"))?;
    }
    Ok(())
}

/// The player's world and the frame position, when the world is in the player's update.
fn position() -> Option<(usize, u8, u16)> {
    let player = crate::dump_types::player();
    if player == 0 {
        return None;
    }
    let world = read::<usize>(player + 0x78)?;
    let key = read::<u32>(player + 0x88)?;
    if read::<u32>(world + 0x678)? != key {
        return None;
    }
    Some((
        world,
        read::<u8>(world + 0x67c)?,
        read::<u16>(world + 0x67e)?,
    ))
}

fn ghidra(addr: usize) -> usize {
    MAIN.get().map_or(addr, |m| m.to_ghidra(addr))
}

unsafe extern "win64" fn on_check(_regs: *mut Registers, _user: usize) {
    let _guard = crate::hook_guard!("phase_log::on_check");
    if let Some((_, comp, phase)) = position() {
        log::info!("SCHK {} {comp} {phase}", crate::dump_types::player_frame());
    }
}

unsafe extern "win64" fn on_end(regs: *mut Registers, _user: usize) {
    let _guard = crate::hook_guard!("phase_log::on_end");
    let r = unsafe { &*regs };
    if let Some((_, comp, phase)) = position() {
        let ra = read::<usize>(r.rsp as usize).unwrap_or(0);
        log::info!(
            "SEND {} {comp} {phase} {} {} {:#x}",
            crate::dump_types::player_frame(),
            r.rdx as u8,
            r.r8 as u8,
            ghidra(ra)
        );
    }
}

unsafe extern "win64" fn on_packet(regs: *mut Registers, _user: usize) {
    let _guard = crate::hook_guard!("phase_log::on_packet");
    let r = unsafe { &*regs };
    let packet = r.rdx as usize;
    let Some((_, comp, phase)) = position() else {
        return;
    };
    let (Some(k), Some(c), Some(s), Some(vt)) = (
        read::<u32>(packet + 0x14),
        read::<u8>(packet + 0x18),
        read::<u16>(packet + 0x1a),
        read::<usize>(packet),
    ) else {
        return;
    };
    let player = crate::dump_types::player();
    if read::<u32>(player + 0x88) == Some(k) {
        log::info!(
            "DRN {} {comp} {phase} key={c},{s} {:#x}",
            crate::dump_types::player_frame(),
            ghidra(vt)
        );
    }
}

unsafe extern "win64" fn on_phase(regs: *mut Registers, _user: usize) {
    let _guard = crate::hook_guard!("phase_log::on_phase");
    let r = unsafe { &*regs };
    if r.rdx as usize != crate::dump_types::player() {
        return;
    }
    if let Some((_, comp, phase)) = position() {
        let ra = read::<usize>(r.rsp as usize).unwrap_or(0);
        log::info!(
            "PH {} {comp} {phase} {:#x}",
            crate::dump_types::player_frame(),
            ghidra(ra)
        );
    }
}

unsafe extern "win64" fn on_tol_stat(regs: *mut Registers, _user: usize) {
    let _guard = crate::hook_guard!("phase_log::on_tol_stat");
    let r = unsafe { &*regs };
    tol(r, &format!("{}", r.rax as u32 as i32));
}

unsafe extern "win64" fn on_tol_2pi(regs: *mut Registers, _user: usize) {
    let _guard = crate::hook_guard!("phase_log::on_tol_2pi");
    let r = unsafe { &*regs };
    tol(r, "2pi");
}

/// Logs a player action's facing tolerance; `rbx` is the action, the return address is at
/// `rsp + 0x28` (push rbx, sub rsp 0x20).
fn tol(r: &Registers, what: &str) {
    let act = r.rbx as usize;
    let player = crate::dump_types::player();
    let Some(entity) = read::<usize>(act + 0xf0).and_then(|c| read::<usize>(c + 8)) else {
        return;
    };
    if player == 0 || entity != player {
        return;
    }
    let ra = read::<usize>(r.rsp as usize + 0x28).unwrap_or(0);
    let sid = read::<usize>(act + 0xf8)
        .filter(|&k| k > 0x10000)
        .and_then(|k| read::<u32>(k + 0x40))
        .unwrap_or(0);
    log::info!(
        "TOL {} {what} {:#x} {:#x} id={sid:#x} ty={:#06x}",
        crate::dump_types::player_frame(),
        ghidra(ra),
        read::<u8>(act + 0x142).unwrap_or(0),
        read::<u16>(act + 0x120).unwrap_or(0)
    );
}

unsafe extern "win64" fn on_set_facing(regs: *mut Registers, _user: usize) {
    let _guard = crate::hook_guard!("phase_log::on_set_facing");
    let r = unsafe { &*regs };
    let pos = r.rbx as usize;
    let player = crate::dump_types::player();
    if player == 0 || read::<usize>(pos + 8) != Some(player) {
        return;
    }
    let (comp, phase) = position().map_or((255, 0), |(_, c, p)| (c, p));
    let ra = read::<usize>(r.rsp as usize + 0x168).unwrap_or(0);
    // 0x141e50940: push rdi, sub rsp 0x20, so its return address is 0x30 above ours.
    let ra2 = if ghidra(ra) == 0x141e50a5c {
        ghidra(read::<usize>(r.rsp as usize + 0x198).unwrap_or(0))
    } else {
        0
    };
    log::info!(
        "FACE {} {comp} {phase} {:.4} {:.4} {:#x} {ra2:#x}",
        crate::dump_types::player_frame(),
        f32::from_bits(r.xmm0 as u32),
        read::<f32>(pos + 0x44c).unwrap_or(0.0),
        ghidra(ra)
    );
}
