//! The server command registry as the overlay receives it (`cmdspec` lines of the debug link,
//! one JSON object per line, docs/protocol/side-channel.md), and the overlay pages a command can
//! name. The server builds these from its registry (`instance-server/src/commands.rs`).

use serde::{Deserialize, Serialize};

/// The Debug tabs a command's `overlay` field may name. The server's registry test checks every
/// built-in command against this list.
pub const DEBUG_PAGES: &[&str] = &[
    "Debug>Help",
    "Debug>Console",
    "Debug>Character",
    "Debug>Stats",
    "Debug>Items",
    "Debug>World",
    "Debug>Monsters",
    "Debug>Life",
    "Debug>Players",
];

/// One positional argument.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CmdArg {
    pub name: String,
    /// `int`, `float`, `text`, `rest`, `bool`, `duration`, `choice=a|b`, `area`, `seed`,
    /// `topology`, `orientation`, `instance`, `player`, or a name list (`class`, `ascendancy`,
    /// `item_base`, `unique`, `mod`, `stat`, `currency`, `gem`, `monster`, `league`, `waypoint`,
    /// `rarity`, `damage_type`).
    pub kind: String,
    pub optional: bool,
}

/// One `--flag`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CmdFlag {
    pub name: String,
    /// The value's kind (as [`CmdArg::kind`]); `None`: a switch without a value.
    pub kind: Option<String>,
    pub help: String,
}

/// One command.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CmdSpec {
    /// `["instance", "join"]`.
    pub path: Vec<String>,
    pub aliases: Vec<String>,
    pub category: String,
    /// The account needs the admin role.
    pub admin: bool,
    pub args: Vec<CmdArg>,
    pub flags: Vec<CmdFlag>,
    pub summary: String,
    pub help: String,
    pub examples: Vec<String>,
    /// The Debug tab that has its form ([`DEBUG_PAGES`]).
    pub overlay: Option<String>,
}

impl CmdSpec {
    /// `instance join`.
    pub fn name(&self) -> String {
        self.path.join(" ")
    }

    /// `instance join <id> [--flag <kind>]`, without the chat prefix.
    pub fn usage(&self) -> String {
        let mut out = self.name();
        for a in &self.args {
            if a.optional {
                out += &format!(" [{}]", a.name);
            } else {
                out += &format!(" <{}>", a.name);
            }
        }
        for f in &self.flags {
            match &f.kind {
                Some(k) => out += &format!(" [--{} <{k}>]", f.name),
                None => out += &format!(" [--{}]", f.name),
            }
        }
        out
    }
}

/// A word as the command parser reads it back: words with spaces get quotes.
pub fn quote(word: &str) -> String {
    if word.is_empty() || word.contains(char::is_whitespace) || word.contains('"') {
        format!("\"{}\"", word.replace('"', "\\\""))
    } else {
        word.to_owned()
    }
}

/// The line a form sends: arguments in order; an empty optional argument followed by a filled
/// one gets the kind's "default" word, trailing empty ones are left out. Words with spaces get
/// quotes, except a `rest` argument, which the server takes as typed; so flags go before it.
/// `flags` holds the values in `spec.flags` order (a switch is set when not empty).
pub fn form_line(c: &CmdSpec, args: &[String], flags: &[String]) -> String {
    let mut words: Vec<&str> = args.iter().map(|w| w.trim()).collect();
    while words.last().is_some_and(|w| w.is_empty()) {
        words.pop();
    }
    let mut out = vec![c.name()];
    let mut rest = None;
    for (w, a) in words.iter().zip(&c.args) {
        if a.kind == "rest" {
            rest = Some((*w).to_owned());
        } else if w.is_empty() {
            out.push(match a.kind.as_str() {
                "seed" => "random".to_owned(),
                "topology" | "orientation" => "natural".to_owned(),
                _ => "-".to_owned(),
            });
        } else {
            out.push(quote(w));
        }
    }
    for (f, v) in c.flags.iter().zip(flags) {
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        out.push(format!("--{}", f.name));
        if f.kind.is_some() {
            out.push(quote(v));
        }
    }
    out.extend(rest);
    out.join(" ")
}

/// What the Console can offer for the word being typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Suggest {
    Nothing,
    /// Words known here: command names, flags, choices.
    Words(Vec<String>),
    /// A name list the server has: ask it with `complete <kind> <typed>`.
    Remote {
        kind: String,
        typed: String,
    },
}

/// The word under the cursor and where it starts in the line (the cursor is at the end).
pub struct Typed<'a> {
    pub start: usize,
    pub word: &'a str,
    pub suggest: Suggest,
}

/// Kinds the server answers `complete` for.
const REMOTE: &[&str] = &[
    "area",
    "player",
    "instance",
    "class",
    "ascendancy",
    "item_base",
    "unique",
    "mod",
    "stat",
    "currency",
    "gem",
    "monster",
    "league",
    "waypoint",
    "rarity",
    "damage_type",
];

/// Reads a partly typed line (without the chat prefix) against the registry: what comes next.
pub fn suggest<'a>(cmds: &[CmdSpec], line: &'a str) -> Typed<'a> {
    let (done, start) = finished_words(line);
    let word = &line[start..];
    let word_clean = word.trim_start_matches('"');
    let lower = word_clean.to_lowercase();
    let starting = |names: Vec<String>| {
        let mut names: Vec<String> = names
            .into_iter()
            .filter(|n| n.to_lowercase().starts_with(&lower))
            .collect();
        names.dedup();
        Suggest::Words(names)
    };
    let suggest = 'found: {
        let Some(first) = done.first() else {
            let mut names: Vec<String> = Vec::new();
            for c in cmds {
                names.extend(c.path.first().cloned());
                names.extend(c.aliases.iter().cloned());
            }
            names.sort();
            break 'found starting(names);
        };
        let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
        // A two-word path, then a one-word path, then an alias.
        let two = done.get(1).and_then(|sub| {
            cmds.iter()
                .find(|c| c.path.len() == 2 && same(&c.path[0], first) && same(&c.path[1], sub))
        });
        let (spec, used) = if let Some(c) = two {
            (c, 2)
        } else if let Some(c) = cmds
            .iter()
            .find(|c| c.path.len() == 1 && same(&c.path[0], first))
        {
            (c, 1)
        } else if let Some(c) = cmds
            .iter()
            .find(|c| c.aliases.iter().any(|a| same(a, first)))
        {
            // Still typing a group's subcommand: an alias that is also a group word.
            let subs: Vec<String> = cmds
                .iter()
                .filter(|o| o.path.len() == 2 && same(&o.path[0], first))
                .map(|o| o.path[1].clone())
                .collect();
            if done.len() == 1 && !subs.is_empty() && !word.is_empty() {
                break 'found starting(subs);
            }
            (c, 1)
        } else {
            let subs: Vec<String> = cmds
                .iter()
                .filter(|o| o.path.len() == 2 && same(&o.path[0], first))
                .map(|o| o.path[1].clone())
                .collect();
            break 'found if done.len() == 1 {
                starting(subs)
            } else {
                Suggest::Nothing
            };
        };
        // The arguments so far: flags with their values take no position.
        let mut position = 0;
        let mut value_of: Option<&str> = None;
        for w in &done[used..] {
            if value_of.take().is_some() {
                continue;
            }
            if let Some(name) = w.strip_prefix("--") {
                value_of = spec
                    .flags
                    .iter()
                    .find(|f| same(&f.name, name) && !name.contains('='))
                    .and_then(|f| f.kind.as_deref());
                continue;
            }
            position += 1;
        }
        if let Some(kind) = value_of {
            break 'found by_kind(kind, word_clean, &lower);
        }
        if word.starts_with("--") {
            let wanted = word.trim_start_matches('-').to_lowercase();
            break 'found Suggest::Words(
                spec.flags
                    .iter()
                    .filter(|f| f.name.to_lowercase().starts_with(&wanted))
                    .map(|f| format!("--{}", f.name))
                    .collect(),
            );
        }
        match spec.args.get(position) {
            Some(a) => by_kind(&a.kind, word_clean, &lower),
            None => Suggest::Words(spec.flags.iter().map(|f| format!("--{}", f.name)).collect()),
        }
    };
    Typed {
        start,
        word,
        suggest,
    }
}

fn by_kind(kind: &str, typed: &str, lower: &str) -> Suggest {
    if let Some(choices) = kind.strip_prefix("choice=") {
        return Suggest::Words(
            choices
                .split('|')
                .filter(|c| c.to_lowercase().starts_with(lower))
                .map(str::to_owned)
                .collect(),
        );
    }
    if kind == "bool" {
        return Suggest::Words(
            ["on", "off"]
                .iter()
                .filter(|c| c.starts_with(lower))
                .map(|c| (*c).to_owned())
                .collect(),
        );
    }
    if REMOTE.contains(&kind) {
        return Suggest::Remote {
            kind: kind.to_owned(),
            typed: typed.to_owned(),
        };
    }
    Suggest::Nothing
}

/// The words before the one being typed, and where that one starts.
fn finished_words(line: &str) -> (Vec<String>, usize) {
    let mut done = Vec::new();
    let mut i = 0;
    let bytes = line.as_bytes();
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        let mut word = String::new();
        let quoted = bytes.get(i) == Some(&b'"');
        if quoted {
            i += 1;
        }
        while let Some(c) = line[i..].chars().next() {
            if quoted && c == '"' {
                i += 1;
                break;
            }
            if !quoted && c.is_whitespace() {
                break;
            }
            word.push(c);
            i += c.len_utf8();
        }
        if i >= line.len() {
            return (done, start);
        }
        done.push(word);
    }
}

/// The longest start all `names` share, ignoring case; spelled like the first name.
pub fn common_prefix(names: &[&str]) -> String {
    let Some(first) = names.first() else {
        return String::new();
    };
    let mut end = first.len();
    for n in &names[1..] {
        let shared: usize = first
            .chars()
            .zip(n.chars())
            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
            .map(|(a, _)| a.len_utf8())
            .sum();
        end = end.min(shared);
    }
    first[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_lists_arguments_and_flags() {
        let spec = CmdSpec {
            path: vec!["item".into(), "give".into()],
            args: vec![
                CmdArg {
                    name: "base".into(),
                    kind: "item_base".into(),
                    optional: false,
                },
                CmdArg {
                    name: "count".into(),
                    kind: "int".into(),
                    optional: true,
                },
            ],
            flags: vec![
                CmdFlag {
                    name: "ilvl".into(),
                    kind: Some("int".into()),
                    help: String::new(),
                },
                CmdFlag {
                    name: "ground".into(),
                    kind: None,
                    help: String::new(),
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            spec.usage(),
            "item give <base> [count] [--ilvl <int>] [--ground]"
        );
    }

    #[test]
    fn quote_only_when_needed() {
        assert_eq!(quote("Iron"), "Iron");
        assert_eq!(quote("Iron Ring"), "\"Iron Ring\"");
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
    }

    fn arg(name: &str, kind: &str, optional: bool) -> CmdArg {
        CmdArg {
            name: name.into(),
            kind: kind.into(),
            optional,
        }
    }

    fn give() -> CmdSpec {
        CmdSpec {
            path: vec!["item".into(), "give".into()],
            args: vec![
                arg("base", "item_base", false),
                arg("seed", "seed", true),
                arg("note", "rest", true),
            ],
            flags: vec![
                CmdFlag {
                    name: "ilvl".into(),
                    kind: Some("int".into()),
                    help: String::new(),
                },
                CmdFlag {
                    name: "rarity".into(),
                    kind: Some("rarity".into()),
                    help: String::new(),
                },
                CmdFlag {
                    name: "ground".into(),
                    kind: None,
                    help: String::new(),
                },
            ],
            ..Default::default()
        }
    }

    fn registry() -> Vec<CmdSpec> {
        vec![
            give(),
            CmdSpec {
                path: vec!["item".into(), "edit".into()],
                ..Default::default()
            },
            CmdSpec {
                path: vec!["god".into()],
                args: vec![arg("state", "bool", true)],
                ..Default::default()
            },
            CmdSpec {
                path: vec!["instance".into(), "join".into()],
                aliases: vec!["join".into()],
                args: vec![arg("id", "instance", false)],
                ..Default::default()
            },
            CmdSpec {
                path: vec!["account".into()],
                args: vec![arg("action", "choice=list|create", false)],
                ..Default::default()
            },
        ]
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn form_lines_quote_names_fill_gaps_and_put_flags_before_rest() {
        let c = give();
        assert_eq!(
            form_line(&c, &strs(&["Iron Ring", "", ""]), &strs(&["", "", ""])),
            "item give \"Iron Ring\""
        );
        assert_eq!(
            form_line(&c, &strs(&["Ring", "", "a note"]), &strs(&["", "", ""])),
            "item give Ring random a note"
        );
        assert_eq!(
            form_line(&c, &strs(&["Ring", "5", ""]), &strs(&["82", "", "on"])),
            "item give Ring 5 --ilvl 82 --ground"
        );
        assert_eq!(
            form_line(&c, &strs(&["Ring", "5", "my note"]), &strs(&["82", "", ""])),
            "item give Ring 5 --ilvl 82 my note"
        );
    }

    fn words(line: &str) -> Vec<String> {
        match suggest(&registry(), line).suggest {
            Suggest::Words(w) => w,
            other => panic!("{line}: {other:?}"),
        }
    }

    fn remote(line: &str) -> (String, String) {
        match suggest(&registry(), line).suggest {
            Suggest::Remote { kind, typed } => (kind, typed),
            other => panic!("{line}: {other:?}"),
        }
    }

    #[test]
    fn the_console_suggests_what_comes_next() {
        assert_eq!(words("i"), ["instance", "item"]);
        assert_eq!(words("jo"), ["join"]);
        assert_eq!(words("item "), ["give", "edit"]);
        assert_eq!(words("item g"), ["give"]);
        assert_eq!(remote("item give Ir"), ("item_base".into(), "Ir".into()));
        assert_eq!(
            remote("item give \"Iron R"),
            ("item_base".into(), "Iron R".into())
        );
        assert_eq!(words("item give Ring --i"), ["--ilvl"]);
        assert_eq!(
            remote("item give Ring --rarity ra"),
            ("rarity".into(), "ra".into())
        );
        // A flag's value takes no position, a switch has no value.
        assert_eq!(
            remote("item give --ilvl 5 Ri"),
            ("item_base".into(), "Ri".into())
        );
        assert_eq!(
            remote("item give --ground Ri"),
            ("item_base".into(), "Ri".into())
        );
        assert_eq!(words("god o"), ["on", "off"]);
        assert_eq!(words("account "), ["list", "create"]);
        assert_eq!(remote("join 1"), ("instance".into(), "1".into()));
        assert!(words("nope x").is_empty());
        let typed = suggest(&registry(), "item give \"Iron");
        assert_eq!((typed.start, typed.word), (10, "\"Iron"));
    }

    #[test]
    fn common_prefix_ignores_case() {
        assert_eq!(common_prefix(&["item_give", "ITEM_edit"]), "item_");
        assert_eq!(common_prefix(&["teleport"]), "teleport");
        assert_eq!(common_prefix(&[]), "");
    }
}
