//! Research aid (`POE_HOOK_PHASE_LOG=1`): where in the player's frame the sprint's end and the
//! keyed messages run. Read-only, logs at function entry.
//!
//! - `SCHK frame comp phase`: `SprintPart_EndCheck` `0x141b76da0`.
//! - `SEND frame comp phase bonk no_anim ra`: `SprintPart_End` `0x141b773a0`.
//! - `DRN frame comp phase key=comp,seq vt`: packet apply `0x1421343c0` for a packet keyed to the
//!   player. `vt` is the packet's vtable (Ghidra address).
//! - `PH frame comp phase ra`: phase step `0x1421303e0`, before the increment.
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

pub fn init(main: &Module) -> Result<(), String> {
    MAIN.set(*main).ok();
    for (at, f) in [
        (SPRINT_CHECK, on_check as ilhook::x64::JmpBackRoutine),
        (SPRINT_END, on_end as ilhook::x64::JmpBackRoutine),
        (PACKET_APPLY, on_packet as ilhook::x64::JmpBackRoutine),
        (PHASE_STEP, on_phase as ilhook::x64::JmpBackRoutine),
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
    Some((world, read::<u8>(world + 0x67c)?, read::<u16>(world + 0x67e)?))
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
        log::info!("PH {} {comp} {phase} {:#x}", crate::dump_types::player_frame(), ghidra(ra));
    }
}
