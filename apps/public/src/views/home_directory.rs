//! Homepage grouping from the recorded public observation; labels and classifications
//! come from the supplied original directory. See docs/homepage-source-parity.md.
use super::navigation::{DirectoryEntry, directory};
use board_store::Board;

pub struct Group<'a> {
    pub title: &'static str,
    pub entries: Vec<Entry<'a>>,
}
pub struct Entry<'a> {
    pub directory: DirectoryEntry<'a>,
    pub nws: bool,
}

const GROUPS: &[(&str, &[&str])] = &[
    (
        "Japanese Culture",
        &["a", "c", "w", "m", "cgl", "cm", "f", "n", "jp", "vt"],
    ),
    (
        "Video Games",
        &["v", "vg", "vm", "vmg", "vp", "vr", "vrpg", "vst"],
    ),
    (
        "Interests",
        &[
            "co", "g", "tv", "k", "o", "an", "tg", "sp", "xs", "pw", "sci", "his", "int", "out",
            "toy",
        ],
    ),
    (
        "Creative",
        &[
            "i", "po", "p", "ck", "ic", "wg", "lit", "mu", "fa", "3", "gd", "diy", "wsg", "qst",
        ],
    ),
    (
        "Other",
        &[
            "biz", "trv", "fit", "x", "adv", "lgbt", "mlp", "news", "wsr", "vip",
        ],
    ),
    ("Misc.", &["b", "r9k", "pol", "bant", "soc", "s4s"]),
    (
        "Adult",
        &[
            "s", "hc", "hm", "h", "e", "u", "d", "y", "t", "hr", "gif", "aco",
        ],
    ),
];
const SOURCE_SLUGS: &[&str] = &[
    "3", "a", "aco", "adv", "an", "asp", "b", "bant", "biz", "c", "cgl", "ck", "cm", "co", "d",
    "diy", "e", "fa", "fit", "g", "gd", "gif", "h", "hc", "hm", "hr", "i", "ic", "his", "int",
    "jp", "k", "lit", "lgbt", "m", "mlp", "mu", "news", "n", "o", "out", "p", "po", "pol", "pw",
    "qst", "r", "r9k", "s4s", "s", "sci", "soc", "sp", "t", "tg", "toy", "trash", "trv", "tv", "u",
    "v", "vg", "vm", "vmg", "vip", "vp", "vr", "vrpg", "vst", "vt", "w", "wg", "wsg", "wsr", "x",
    "xs", "y", "f",
];
const SOURCE_NWS: &[&str] = &[
    "aco", "b", "bant", "d", "e", "gif", "h", "hc", "hm", "hr", "i", "ic", "pol", "r", "r9k",
    "s4s", "s", "soc", "t", "trash", "u", "wg", "y", "f",
];

pub fn groups(boards: &[Board]) -> Vec<Group<'_>> {
    let mut remaining: Vec<_> = directory(boards)
        .into_iter()
        .map(|entry| {
            let nws = if SOURCE_SLUGS.contains(&entry.board.slug.as_str()) {
                SOURCE_NWS.contains(&entry.board.slug.as_str())
            } else {
                !entry.board.worksafe
            };
            Entry {
                directory: entry,
                nws,
            }
        })
        .collect();
    let mut output = Vec::new();
    for &(title, slugs) in GROUPS {
        let mut entries = Vec::new();
        for &slug in slugs {
            if let Some(index) = remaining
                .iter()
                .position(|entry| entry.directory.board.slug == slug)
            {
                entries.push(remaining.remove(index));
            }
        }
        if !entries.is_empty() {
            output.push(Group { title, entries });
        }
    }
    if !remaining.is_empty() {
        output.push(Group {
            title: "More boards",
            entries: remaining,
        });
    }
    output
}
