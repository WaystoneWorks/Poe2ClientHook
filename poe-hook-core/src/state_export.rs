//! Live state export for unattended tests (`[features] state_export`, `POE_HOOK_STATE`): every
//! `state_export_ms` (default 250) a background thread writes `poe-hook-state.json` next to the
//! exe (temporary file, then rename, so a reader never sees half a file). Format:
//! docs/guides/setup.md "Live state file". Only reads game memory (guarded reads), so it may run
//! on official servers too.
//!
//! Sources (Hotfix 1 addresses, docs/re/addresses.md "UI elements" and "Life component"):
//! - player: `inspect::player()` (Positioned and Actor, the same reads as the overlay), the
//!   simulation frame counter of the entity tick hook;
//! - area: game world `+0x98` → WorldAreas row (`+0` id string pointer, `+0x2a` hash16, `+0x1b4`
//!   kind), name from `poe-hook-data/areas.tsv`;
//! - life pools: the player's `Life` component (reader `0x141d8c260`): life `+0x1a8`, mana
//!   `+0x200`, energy shield `+0x240`; pool `+0x38` current, `+0x34` maximum, `+0x30` reserved (assumed);
//! - panels: every UI element whose visibility changed through SetVisible `0x1427af130`
//!   (hooked here) and that is visible now with all its parents (flags `+0x168` bit 11, parent
//!   `+0xb8`, name `std::wstring` at `+0x98`), as `parent/child` name paths;
//! - packets: the last messages of the packet capture (`packets::since`).

use std::{
    collections::{HashMap, VecDeque},
    fmt::Write as _,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use ilhook::x64::{HookType, Registers};
use poe_hook_shared::mem::read;

static STOP: AtomicBool = AtomicBool::new(false);
static THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
/// UI elements seen by SetVisible: element → its vtable then (a freed element fails the check).
static ELEMENTS: Mutex<Option<HashMap<usize, usize>>> = Mutex::new(None);
const MAX_ELEMENTS: usize = 16384;
const KEEP_PACKETS: usize = 40;

/// UI element layout (SetVisible `0x1427af130`, escape menu constructor `0x1402c4980`).
const UI_FLAGS: usize = 0x168;
const UI_VISIBLE: u32 = 0x800;
const UI_PARENT: usize = 0xb8;
const UI_NAME: usize = 0x98;
/// Life component pools (reader `0x141d8c260`, pool readers `0x141d8bec0` / `0x141d88e70`).
const LIFE_POOLS: [(&str, usize); 3] = [("life", 0x1a8), ("mana", 0x200), ("es", 0x240)];
/// `+0x38` is the value the server sends first (the current value: official servers send it
/// with `+0x30` = 0 for a living player); `+0x34` is the maximum the client works out from the
/// stats; `+0x30` the server's last pool field (assumed reserved). Not confirmed by damage yet.
const POOL_CURRENT: usize = 0x38;
const POOL_MAX: usize = 0x34;
const POOL_RESERVED: usize = 0x30;

pub fn path() -> std::path::PathBuf {
    poe_hook_shared::exe_dir().join("poe-hook-state.json")
}

/// Starts the writer (and the SetVisible hook when its signature resolved).
pub fn init(interval_ms: u64) -> Result<(), String> {
    let _ = std::fs::remove_file(path());
    if crate::sigs::feature_ok("ui_panels") {
        *ELEMENTS.lock().unwrap() = Some(HashMap::new());
        crate::hooks::install(
            crate::sigs::addr("ui_set_visible")?,
            HookType::JmpBack(on_set_visible),
            0,
        )
        .map_err(|e| format!("hook ui_set_visible: {e}"))?;
    } else {
        log::warn!("state export: no UI panels (signature ui_set_visible did not resolve)");
    }
    STOP.store(false, Ordering::Relaxed);
    let interval = Duration::from_millis(interval_ms.clamp(50, 10_000));
    let handle = std::thread::Builder::new()
        .name("poe-hook-state".into())
        .spawn(move || writer(interval))
        .map_err(|e| e.to_string())?;
    *THREAD.lock().unwrap() = Some(handle);
    log::info!(
        "state export: {} every {} ms",
        path().display(),
        interval.as_millis()
    );
    Ok(())
}

pub fn stop() {
    STOP.store(true, Ordering::Relaxed);
    if let Some(h) = THREAD.lock().unwrap().take() {
        let _ = h.join();
    }
}

unsafe extern "win64" fn on_set_visible(regs: *mut Registers, _: usize) {
    let _guard = crate::hook_guard!("state_export::on_set_visible");
    let element = unsafe { (*regs).rcx } as usize;
    let Some(vt) = read::<usize>(element) else {
        return;
    };
    let mut g = ELEMENTS.lock().unwrap();
    let Some(map) = g.as_mut() else {
        return;
    };
    if map.len() >= MAX_ELEMENTS && !map.contains_key(&element) {
        map.retain(|&e, &mut v| read::<usize>(e) == Some(v));
        if map.len() >= MAX_ELEMENTS {
            return;
        }
    }
    map.insert(element, vt);
}

fn writer(interval: Duration) {
    let target = path();
    let tmp = target.with_extension("json.tmp");
    let mut packets = Packets::default();
    let seq = AtomicU64::new(0);
    while !STOP.load(Ordering::Relaxed) {
        let n = seq.fetch_add(1, Ordering::Relaxed) + 1;
        packets.pull();
        let text = state_json(n, &packets);
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &target);
        }
        // Sleep in small steps so `stop` (core reload) does not wait long.
        let mut left = interval;
        while !left.is_zero() && !STOP.load(Ordering::Relaxed) {
            let step = left.min(Duration::from_millis(50));
            std::thread::sleep(step);
            left -= step;
        }
    }
}

#[derive(Default)]
struct Packets {
    last_seq: u64,
    recent: VecDeque<String>,
}

impl Packets {
    fn pull(&mut self) {
        for m in crate::packets::since(self.last_seq) {
            self.last_seq = m.seq;
            let mut s = String::new();
            let _ = write!(
                s,
                "{{\"seq\":{},\"t\":{},\"dir\":\"{}\",\"chan\":\"{}\",\"op\":\"{:#06x}\",\"name\":{},\"len\":{}}}",
                m.seq,
                m.unix_ms,
                m.dir.name(),
                m.channel.name(),
                m.opcode,
                json_str(m.name().unwrap_or("")),
                m.len
            );
            self.recent.push_back(s);
            while self.recent.len() > KEEP_PACKETS {
                self.recent.pop_front();
            }
        }
    }
}

fn state_json(seq: u64, packets: &Packets) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut s = String::with_capacity(4096);
    let _ = write!(
        s,
        "{{\"version\":1,\"seq\":{seq},\"t\":{now},\"read_only\":{},\"frame\":{}",
        crate::host::read_only(),
        crate::dump_types::player_frame()
    );
    s.push_str(",\"area\":");
    s.push_str(&area_json());
    s.push_str(",\"player\":");
    match crate::inspect::player() {
        Some(p) => {
            let action = |a: u16| json_str(crate::gamedata::action_name(a as u32).unwrap_or(""));
            let _ = write!(
                s,
                "{{\"id\":{},\"grid\":[{},{}],\"world\":[{:.2},{:.2}],\"facing\":{:.4},\"action\":{},\"action2\":{},\"wasd\":[{},{}]",
                p.id,
                p.grid.0,
                p.grid.1,
                p.world.0,
                p.world.1,
                p.facing,
                action(p.action),
                action(p.action2),
                p.wasd.0,
                p.wasd.1
            );
            for (name, off) in LIFE_POOLS {
                let _ = write!(s, ",\"{name}\":");
                s.push_str(&pool_json(p.entity, off));
            }
            s.push('}');
        }
        None => s.push_str("null"),
    }
    s.push_str(",\"panels\":[");
    for (i, p) in panels().iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&json_str(p));
    }
    s.push_str("],\"packets\":[");
    for (i, p) in packets.recent.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(p);
    }
    s.push_str("]}\n");
    s
}

fn area_json() -> String {
    let world = crate::dump_types::game_world();
    if world == 0 || !crate::sigs::feature_ok("dump_object_types") {
        return "null".into();
    }
    let o = crate::sigs::off();
    let Some(row) = read::<usize>(world + o.world_area).filter(|&r| r != 0) else {
        return "null".into();
    };
    let id = read::<usize>(row)
        .map(|p| wide_cstr(p, 64))
        .unwrap_or_default();
    let name = crate::areas::find(&id).map(|a| a.name.as_str()).unwrap_or("");
    format!(
        "{{\"id\":{},\"name\":{},\"hash16\":{},\"kind\":{},\"tile_hash\":{}}}",
        json_str(&id),
        json_str(name),
        read::<u16>(row + 0x2a).unwrap_or(0),
        read::<u32>(row + o.area_kind).unwrap_or(0),
        read::<u32>(world + o.world_tile_hash).unwrap_or(0)
    )
}

fn pool_json(entity: usize, off: usize) -> String {
    let Some(life) = component(entity, "Life") else {
        return "null".into();
    };
    let pool = life + off;
    match (
        read::<i32>(pool + POOL_CURRENT),
        read::<i32>(pool + POOL_MAX),
        read::<i32>(pool + POOL_RESERVED),
    ) {
        (Some(cur), Some(max), Some(res)) => {
            format!("{{\"current\":{cur},\"max\":{max},\"reserved\":{res}}}")
        }
        _ => "null".into(),
    }
}

fn component(entity: usize, name: &str) -> Option<usize> {
    let index = crate::entities::component_names(entity)
        .into_iter()
        .find(|(_, n)| n == name)
        .map(|(i, _)| i)?;
    crate::entities::components(entity).get(index).copied()
}

/// Elements visible with all their parents, as `root/.../element` paths of names (`@<vtable>`
/// for an element without a name), sorted, no duplicates.
fn panels() -> Vec<String> {
    let elements: Vec<(usize, usize)> = match ELEMENTS.lock().unwrap().as_ref() {
        Some(m) => m.iter().map(|(&e, &v)| (e, v)).collect(),
        None => return Vec::new(),
    };
    let mut out: Vec<String> = elements
        .into_iter()
        .filter(|&(e, vt)| read::<usize>(e) == Some(vt))
        .filter_map(|(e, _)| {
            let mut names = vec![label(e)];
            let mut cur = e;
            for _ in 0..64 {
                let flags = read::<u32>(cur + UI_FLAGS)?;
                if flags & UI_VISIBLE == 0 {
                    return None;
                }
                let parent = read::<usize>(cur + UI_PARENT)?;
                if parent == 0 {
                    break;
                }
                names.push(label(parent));
                cur = parent;
            }
            names.reverse();
            Some(names.join("/"))
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The element's name, or `@<vtable>` (Ghidra address) when it has none.
fn label(element: usize) -> String {
    let name = ui_name(element);
    if !name.is_empty() {
        return name;
    }
    let vt = read::<usize>(element).unwrap_or(0);
    format!("@{:x}", crate::sigs::ghidra(vt))
}

/// MSVC `std::wstring` at `at`: inline buffer while capacity < 8, else a pointer; size `+0x10`,
/// capacity `+0x18`.
fn ui_name(element: usize) -> String {
    let at = element + UI_NAME;
    let (Some(len), Some(cap)) = (read::<usize>(at + 0x10), read::<usize>(at + 0x18)) else {
        return String::new();
    };
    if len == 0 || len > 128 || cap < len {
        return String::new();
    }
    let buf = if cap > 7 {
        read::<usize>(at).unwrap_or(0)
    } else {
        at
    };
    wide(buf, len)
}

fn wide(p: usize, len: usize) -> String {
    let units: Vec<u16> = (0..len).filter_map(|i| read::<u16>(p + i * 2)).collect();
    if units.len() != len {
        return String::new();
    }
    String::from_utf16_lossy(&units)
}

fn wide_cstr(p: usize, max: usize) -> String {
    let units: Vec<u16> = (0..max)
        .map_while(|i| read::<u16>(p + i * 2).filter(|&u| u != 0))
        .collect();
    String::from_utf16_lossy(&units)
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
