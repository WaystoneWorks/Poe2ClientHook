//! The `alert` block of the debug link (docs/protocol/side-channel.md): the server tells the DLL
//! to show a popup. Only `terrain_mismatch` exists so far. Parsing is here so it can be tested on
//! the host.

/// A terrain mismatch alert (`alert\tterrain_mismatch`, then `key\tvalue` lines up to `end`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TerrainMismatch {
    /// `G1_2 (Clearfell)`.
    pub area: String,
    /// The area id alone (`G1_2`), for file names.
    pub area_id: String,
    pub seed: String,
    pub layout: String,
    /// `0x1a2b3c4d`, or `none` when the client sent no hashes.
    pub tile_client: String,
    pub tile_server: String,
    pub doodad_client: String,
    pub doodad_server: String,
    pub reason: String,
    pub dest: String,
    pub report: String,
}

impl TerrainMismatch {
    pub fn tile_differs(&self) -> bool {
        self.tile_client != self.tile_server
    }

    pub fn doodad_differs(&self) -> bool {
        self.doodad_client != self.doodad_server
    }

    /// Part of the export file names: area, seed and both tile hashes.
    pub fn file_tag(&self) -> String {
        let clean = |s: &str| {
            s.trim_start_matches("0x")
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>()
        };
        format!(
            "{}_{}_client{}_server{}",
            clean(&self.area_id),
            clean(&self.seed),
            clean(&self.tile_client),
            clean(&self.tile_server)
        )
    }
}

/// `client 0x1a2b3c4d  server 0x5e6f7081` -> (client, server).
fn pair(v: &str) -> (String, String) {
    let mut it = v.split_whitespace();
    let (mut client, mut server) = (String::new(), String::new());
    while let Some(word) = it.next() {
        match word {
            "client" => client = it.next().unwrap_or_default().to_string(),
            "server" => server = it.next().unwrap_or_default().to_string(),
            _ => {}
        }
    }
    (client, server)
}

/// Parses a datagram text that starts with `alert\tterrain_mismatch`; `None` for any other text.
pub fn parse_terrain_mismatch(text: &str) -> Option<TerrainMismatch> {
    let mut lines = text.lines();
    if lines.next()? != "alert\tterrain_mismatch" {
        return None;
    }
    let mut a = TerrainMismatch::default();
    for line in lines {
        let Some((k, v)) = line.split_once('\t') else {
            continue;
        };
        match k {
            "area" => {
                a.area = v.to_string();
                a.area_id = v.split_whitespace().next().unwrap_or_default().to_string();
            }
            "seed" => a.seed = v.to_string(),
            "layout" => a.layout = v.to_string(),
            "tile" => (a.tile_client, a.tile_server) = pair(v),
            "doodad" => (a.doodad_client, a.doodad_server) = pair(v),
            "reason" => a.reason = v.to_string(),
            "dest" => a.dest = v.to_string(),
            "report" => a.report = v.to_string(),
            _ => {}
        }
    }
    Some(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "alert\tterrain_mismatch\narea\tG1_2 (Clearfell)\nseed\t123456789\nlayout\ttopology 3, orientation 1\ntile\tclient 0x1a2b3c4d  server 0x5e6f7081\ndoodad\tclient 0x11111111  server 0x11111111\nreason\ttile hash differs\ndest\tprevious instance 7 (G1_1 Riverbank)\nreport\tterrain-mismatch build=0.5.5e.hotfix1 area=G1_2\nend\n";

    #[test]
    fn parses_the_servers_block() {
        let a = parse_terrain_mismatch(TEXT).unwrap();
        assert_eq!(a.area, "G1_2 (Clearfell)");
        assert_eq!(a.area_id, "G1_2");
        assert_eq!(a.seed, "123456789");
        assert_eq!(
            (a.tile_client.as_str(), a.tile_server.as_str()),
            ("0x1a2b3c4d", "0x5e6f7081")
        );
        assert!(a.tile_differs());
        assert!(!a.doodad_differs());
        assert_eq!(a.dest, "previous instance 7 (G1_1 Riverbank)");
        assert_eq!(a.file_tag(), "G1_2_123456789_client1a2b3c4d_server5e6f7081");
    }

    #[test]
    fn other_text_is_not_an_alert() {
        assert!(parse_terrain_mismatch("tick\t5\nserver_pos\t1 2\n").is_none());
        assert!(parse_terrain_mismatch("alert\tsomething_else\n").is_none());
    }

    #[test]
    fn no_hashes_from_the_client_differ() {
        let a = parse_terrain_mismatch("alert\tterrain_mismatch\ntile\tclient none  server 0x00000001\ndoodad\tclient none  server none\n").unwrap();
        assert!(a.tile_differs());
        assert!(!a.doodad_differs());
    }
}
