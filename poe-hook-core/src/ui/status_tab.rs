//! Status tab: mode and read-only latch, session folder, core reload, connections, Vulkan.

use imgui::Ui;

use super::widgets::{self, Tone};

fn mode_line() -> String {
    let cfg = crate::config();
    format!(
        "mode {:?}, read-only {}",
        cfg.mode,
        if crate::host::read_only() {
            "ON"
        } else {
            "off"
        }
    )
}

pub fn draw(ui: &Ui) {
    widgets::header(
        ui,
        "Mode, connections, feature health and loaded mods.",
        "Private = our server. Official read-only = the DLL only observes and logs.",
    );
    let cfg = &crate::config().overlay;
    let read_only = crate::host::read_only();
    let conns = crate::host::connections();
    // Chips: the mode, the role, the position sync.
    if read_only {
        widgets::chip(ui, "OFFICIAL read-only", Tone::Warn);
    } else {
        widgets::chip(ui, "PRIVATE", Tone::Ok);
    }
    if let Some(me) = crate::devlink::me() {
        ui.same_line();
        if me.admin {
            widgets::chip(ui, "ADMIN", Tone::Warn);
        } else {
            widgets::chip(ui, "player", Tone::Muted);
        }
    }
    if !read_only {
        ui.same_line();
        match crate::devlink::state() {
            Some((age, _)) if age.as_millis() < 2000 => widgets::chip(ui, "sync ok", Tone::Ok),
            Some(_) => widgets::chip(ui, "sync stale", Tone::Warn),
            None => widgets::chip(ui, "no sync", Tone::Muted),
        }
    }
    if !crate::sigs::verified() {
        ui.same_line();
        widgets::chip(ui, "build NOT verified", Tone::Bad);
    }
    if let Some(c) = conns.last() {
        ui.same_line();
        ui.text(format!(
            "{}.{}.{}.{}:{}",
            c.ip[0], c.ip[1], c.ip[2], c.ip[3], c.port
        ));
    }
    if let Some((_, list, _)) = crate::devlink::instances()
        && let Some(i) = list.iter().find(|i| i.current)
    {
        ui.same_line();
        let name = crate::areas::find(&i.area).map_or("", |a| a.name.as_str());
        ui.text(format!("{name} ({})", i.area));
    }
    ui.text(format!(
        "{:.0} fps   core {} (load #{}{})   session {}",
        ui.io().framerate(),
        env!("CARGO_PKG_VERSION"),
        crate::host::reload_count(),
        "",
        crate::host::session_dir()
            .file_name()
            .map_or(String::new(), |n| n.to_string_lossy().into_owned())
    ));
    if ui.button("Reload core") {
        crate::host::request_reload();
    }
    ui.same_line();
    if ui.button("Snapshot") {
        crate::snapshot::request();
    }
    ui.same_line();
    if ui.button("Hide overlay") {
        crate::overlay::set_visible(false);
    }
    ui.same_line();
    ui.text_disabled(format!(
        "{} shows/hides, {} saves a snapshot",
        super::key_name(cfg.toggle_key),
        super::key_name(cfg.snapshot_key)
    ));
    if read_only {
        ui.text_colored(
            super::theme::WARN,
            "Nothing is sent, altered or injected; the DLL only observes.",
        );
    }
    ui.separator();
    if ui.collapsing_header(
        format!("Connections ({})##conns", conns.len()),
        imgui::TreeNodeFlags::DEFAULT_OPEN,
    ) {
        if conns.is_empty() {
            widgets::empty(ui, "No connection yet.");
        }
        if let Some(_t) = ui.begin_table_header_with_flags(
            "conns2",
            ["time", "kind", "address", "socket", "trusted"].map(imgui::TableColumnSetup::new),
            crate::ui::table_opts(),
        ) {
            for c in conns.iter().rev() {
                ui.table_next_column();
                ui.text(crate::util::clock(c.unix_ms));
                ui.table_next_column();
                ui.text(if c.external { "external" } else { "local" });
                ui.table_next_column();
                ui.text(format!(
                    "{}.{}.{}.{}:{}",
                    c.ip[0], c.ip[1], c.ip[2], c.ip[3], c.port
                ));
                ui.table_next_column();
                ui.text(format!("{:#x}", c.socket));
                ui.table_next_column();
                if c.trusted() {
                    ui.text_colored(super::theme::OK, "yes");
                } else {
                    ui.text_colored(super::theme::BAD, "no");
                }
            }
        }
    }
    let health = crate::sigs::feature_health();
    let off: Vec<_> = health.iter().filter(|(_, ok, _)| !ok).collect();
    let title = if off.is_empty() {
        format!("Features: {} ok", health.len())
    } else {
        format!(
            "Features: {} ok, {} OFF ({})",
            health.len() - off.len(),
            off.len(),
            off.iter()
                .map(|(n, ..)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let _c =
        (!off.is_empty()).then(|| ui.push_style_color(imgui::StyleColor::Text, super::theme::BAD));
    let open = ui.collapsing_header(format!("{title}##features"), imgui::TreeNodeFlags::empty());
    drop(_c);
    if open {
        for (name, ok, problems) in &health {
            if *ok {
                ui.text_colored(super::theme::OK, format!("ok   {name}"));
            } else {
                ui.text_colored(
                    super::theme::BAD,
                    format!("OFF  {name}: {}", problems.join("; ")),
                );
            }
        }
        if health.is_empty() {
            widgets::empty(ui, "Signature table not loaded.");
        }
    }
    let mods = crate::host::mod_report()
        .and_then(|b| poe_hook_shared::modreport::ModReport::decode(&b).ok())
        .map(|r| r.mods)
        .unwrap_or_default();
    if ui.collapsing_header(
        format!("Loaded mods ({})##mods", mods.len()),
        imgui::TreeNodeFlags::empty(),
    ) {
        if mods.is_empty() {
            widgets::empty(ui, "No mods loaded.");
        }
        for m in &mods {
            ui.text(format!("{} {}", m.id, m.version));
        }
    }
    ui.separator();
    ui.text_disabled(vulkan_line());
}

fn vulkan_line() -> String {
    match crate::host::vulkan() {
        Some(v) => format!(
            "Vulkan: swapchain {}x{} format {:?}, {} images, usage {:#x}",
            v.width,
            v.height,
            ash::vk::Format::from_raw(v.format),
            v.image_count,
            v.usage
        ),
        None => "Vulkan: no swapchain yet".into(),
    }
}

pub fn state_text() -> String {
    let mut s = String::new();
    s.push_str(&mode_line());
    s.push('\n');
    s.push_str(&format!(
        "debug core {} (load #{})\n",
        env!("CARGO_PKG_VERSION"),
        crate::host::reload_count()
    ));
    s.push_str(&vulkan_line());
    s.push('\n');
    for c in crate::host::connections() {
        s.push_str(&format!(
            "connection #{} socket {:#x} {}.{}.{}.{}:{} {} at {}\n",
            c.seq,
            c.socket,
            c.ip[0],
            c.ip[1],
            c.ip[2],
            c.ip[3],
            c.port,
            if c.external { "external" } else { "local" },
            crate::util::clock(c.unix_ms)
        ));
    }
    s.push_str(&crate::inspect::player_text());
    s
}
