//! Authenticated side channel to our instance server (private mode only): the session keys,
//! the sealed UDP frames of the position reports and the debug link, and the mod report.
//!
//! The keys come from the instance session's 64-byte key material, which the game passes to
//! `0x14212ac10` (the packet-cipher builder of the instance `EnableEncryption` handler, with
//! `InstanceDetails + 0x38` in `rdx`). The hook below copies it, derives the session keys
//! (`poe_hook_shared::sidechannel`, the same code the server runs) and never logs it. Every
//! new instance connection brings new material, hence a new session.
//!
//! Nothing here sends to an untrusted address or while the read-only latch is on:
//! [`allowed`] (`trust::side_channel_allowed`) is the gate of every send, and without a session
//! (the hook does not capture on official servers) there is nothing to seal with anyway.

use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    sync::Mutex,
    time::Duration,
};

use ilhook::x64::{HookType, Registers};
use poe_hook_shared::{
    module::Module,
    sidechannel::{self as sc, Dir, Kind, ReplayWindow, SessionKeys},
    trust,
};

struct Session {
    keys: SessionKeys,
    /// Counter of the UDP stream (reports and commands).
    udp: u64,
    /// Counter of the TCP side stream (the mod report; separate from the UDP stream).
    stream: u64,
    /// Replay window of the server's debug frames.
    debug: ReplayWindow,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// Hooks the EnableEncryption case of the session handler, where it builds the instance packet
/// ciphers (signature `instance_ciphers`; rbx = the stored InstanceDetails).
pub fn init(_main: &Module) -> Result<(), String> {
    crate::hooks::install(
        crate::sigs::addr("instance_ciphers")?,
        HookType::JmpBack(on_instance_ciphers),
        0,
    )
    .map_err(|e| format!("hook: {e}"))
}

unsafe extern "win64" fn on_instance_ciphers(regs: *mut Registers, _: usize) {
    let _guard = crate::hook_guard!("side::on_instance_ciphers");
    let r = unsafe { &*regs };
    // Official servers (latched, or any untrusted connection) never get a session.
    if crate::host::read_only() || !crate::ui::custom_server() {
        return;
    }
    let addr = r.rbx as usize + crate::sigs::off().session_key_material;
    if !poe_hook_shared::mem::readable(addr, 64) {
        return;
    }
    let mut material = [0u8; 64];
    unsafe { std::ptr::copy_nonoverlapping(addr as *const u8, material.as_mut_ptr(), 64) };
    let keys = SessionKeys::derive(&material);
    log::info!("side channel: session {:016x}", keys.id);
    let id = keys.id;
    *SESSION.lock().unwrap() = Some(Session {
        keys,
        udp: 0,
        stream: 0,
        debug: ReplayWindow::default(),
    });
    report_mods(id);
}

/// Tells the server which client mods are loaded (`Kind::ModReport`, TCP on the instance
/// port), from a background thread: it waits until the loader has finished loading mods and the
/// connection to a trusted server is known, and gives up after about a minute. A private server
/// that requires mods compares the list at login (docs/protocol/side-channel.md). Never sent
/// without a session, so never to an official server or while read-only.
fn report_mods(session: u64) {
    crate::hooks::spawn(move || {
        for attempt in 1..=60 {
            if session_id() != Some(session) {
                return; // a newer instance connection took over
            }
            let addr = crate::position_sync::server_addr();
            let payload = crate::host::mod_report();
            if let (Some(dest), Some(payload)) = (addr, payload) {
                match try_report(dest, &payload) {
                    Ok(msg) => {
                        log::info!("mod report to the server: {msg}");
                        return;
                    }
                    Err(e) => log::warn!("mod report (attempt {attempt}): {e}"),
                }
            }
            if !crate::hooks::sleep(Duration::from_secs(1)) {
                return;
            }
        }
        log::warn!(
            "mod report: not sent (no trusted server connection or the loader never finished)"
        );
    });
}

/// One report connection: the sealed frame, then the server's status.
fn try_report(dest: ([u8; 4], u16), payload: &[u8]) -> Result<String, String> {
    if !allowed(dest.0, dest.1) {
        return Err("server not trusted".into());
    }
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(dest.0)), dest.1);
    let frame = {
        let mut guard = SESSION.lock().unwrap();
        let s = guard.as_mut().ok_or("no session")?;
        s.stream += 1;
        sc::seal(&s.keys, Kind::ModReport, s.stream, payload)
    };
    let mut tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|e| format!("connect {addr}: {e}"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(15))).ok();
    tcp.set_write_timeout(Some(Duration::from_secs(15))).ok();
    let mut msg = Vec::with_capacity(8 + frame.len());
    msg.extend_from_slice(&sc::STREAM_MAGIC);
    msg.extend_from_slice(&(frame.len() as u32).to_le_bytes());
    msg.extend_from_slice(&frame);
    tcp.write_all(&msg).map_err(|e| format!("send: {e}"))?;
    let (status, text) = reply(&mut tcp)?;
    if status == sc::REPLY_OK {
        Ok(text)
    } else {
        Err(format!("refused ({status}): {text}"))
    }
}

/// Whether anything may be sent to `ip:port` (see the module docs).
pub fn allowed(ip: [u8; 4], port: u16) -> bool {
    trust::side_channel_allowed(
        crate::host::read_only(),
        &SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), port),
    )
}

/// The current session id, if the instance connection gave us one.
pub fn session_id() -> Option<u64> {
    SESSION.lock().unwrap().as_ref().map(|s| s.keys.id)
}

/// A sealed UDP frame of `kind` (client to server), with the next counter.
pub fn seal_udp(kind: Kind, payload: &[u8]) -> Option<Vec<u8>> {
    let mut guard = SESSION.lock().unwrap();
    let s = guard.as_mut()?;
    s.udp += 1;
    Some(sc::seal(&s.keys, kind, s.udp, payload))
}

/// The payload of a server debug frame, if it is for our session, authentic and fresh.
pub fn open_debug(frame: &[u8]) -> Option<Vec<u8>> {
    let mut guard = SESSION.lock().unwrap();
    let s = guard.as_mut()?;
    let f = sc::open(&s.keys, Dir::ServerToClient, frame).ok()?;
    if f.kind != Kind::Debug || !s.debug.accept(f.counter) {
        return None;
    }
    Some(f.payload.to_vec())
}

/// `PSUR`, `u8 status`, `u16 length`, text.
fn reply(tcp: &mut TcpStream) -> Result<(u8, String), String> {
    let mut head = [0u8; 7];
    tcp.read_exact(&mut head)
        .map_err(|e| format!("reply: {e}"))?;
    if head[..4] != sc::STREAM_REPLY_MAGIC {
        return Err("not a side stream reply".into());
    }
    let mut text = vec![0u8; u16::from_le_bytes([head[5], head[6]]) as usize];
    tcp.read_exact(&mut text)
        .map_err(|e| format!("reply: {e}"))?;
    Ok((head[4], String::from_utf8_lossy(&text).into_owned()))
}
