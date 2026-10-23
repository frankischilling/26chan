//! Pure event presentation rules from the supplied source. These helpers do not
//! activate dormant endpoints, grant perks, or treat event balances as money.

pub mod credentials;
pub mod emotes;
pub mod market;

pub const STOCKS: [&str; 18] = [
    "PEPE", "WOJK", "ANIME", "CHAD", "CLOWN", "LOL", "SICP", "AUTSM", "BANE", "CIA", "BOOB",
    "RDDT", "DESU", "JANNY", "GME", "CHUCK", "YTSB", "GACHI",
];
pub const PERKS: [&str; 19] = [
    "smiley", "sad", "ok", "animal", "food", "check", "cross", "like", "unlove", "rage", "perfect",
    "onsen", "heart", "joy", "pig", "dog", "cat", "rainbow", "frog",
];

/// Historical April 2024 HTML identity encoding, not a Pass purchase year.
pub fn stock_identity(value: i64) -> Option<&'static str> {
    value
        .checked_sub(10_000)
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| STOCKS.get(index).copied())
}

pub fn investor_name(net_worth: i64) -> &'static str {
    match net_worth {
        ..500 => "Destitute Investor",
        500..1500 => "Helpless Investor",
        1500..5000 => "Poor Investor",
        5000..50_000 => "Fledgling Investor",
        50_000..500_000 => "Aspiring Investor",
        500_000..2_000_000 => "Rich Investor",
        2_000_000..5_000_000 => "Anonymous Magnate",
        _ => "Anonymous Mogul",
    }
}

/// The source helper is inside a disabled historical block. Calling this pure
/// function does not enable scoring or add a badge to an ordinary post.
pub fn halloween_class(score: i64) -> &'static str {
    match score {
        ..25 => "",
        25..50 => " n-jol-1",
        50..100 => " n-jol-2",
        100..200 => " n-jol-3",
        200..500 => " n-jol-4",
        500..1000 => " n-jol-5",
        _ => " n-jol-6",
    }
}

/// All active thresholds in this snapshot are zero. Commented-out options do
/// not become available at a higher score.
pub fn unlocked_perks(score: i64) -> &'static [&'static str] {
    if score >= 0 { &PERKS } else { &[] }
}

/// The first recognized, literal-space-delimited option wins. The snapshot's
/// score getter returns zero and its showscore option is not in the active map.
pub fn selected_perk(options: &str) -> Option<&'static str> {
    if options.len() > 100 {
        return None;
    }
    options
        .split(' ')
        .find_map(|value| PERKS.iter().copied().find(|perk| *perk == value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_and_options_match_executed_source_helpers() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/event-reference.json")).unwrap();
        let rows = &fixture["cases"];
        for row in rows["stock"].as_array().unwrap() {
            let stock = stock_identity(row["value"].as_i64().unwrap());
            assert_eq!(stock, row["stock"].as_str(), "{row}");
            assert_eq!(
                stock.map_or_else(String::new, |stock| format!(" p-xa24-{stock}")),
                row["class"]
            );
            assert_eq!(
                stock.map_or_else(String::new, |stock| format!(
                    " <span data-tip=\"{stock}\" class=\"n-xa24 n-xa24-{stock}\"></span>"
                )),
                row["badge"]
            );
        }
        for row in rows["investor"].as_array().unwrap() {
            assert_eq!(investor_name(row["value"].as_i64().unwrap()), row["name"]);
        }
        for row in rows["halloween"].as_array().unwrap() {
            assert_eq!(
                halloween_class(row["value"].as_i64().unwrap()),
                row["class"]
            );
        }
        for row in rows["perks"].as_array().unwrap() {
            let unlocked = unlocked_perks(row["score"].as_i64().unwrap());
            assert_eq!(serde_json::to_value(unlocked).unwrap(), row["unlocked"]);
            for perk in PERKS {
                assert_eq!(
                    unlocked.contains(&perk),
                    row["state"][perk].as_bool().unwrap()
                );
            }
        }
        for row in rows["options"].as_array().unwrap() {
            assert_eq!(
                selected_perk(row["value"].as_str().unwrap()),
                row["result"][1].as_str()
            );
            assert_eq!(row["result"][0], false);
        }
    }

    #[test]
    fn integer_edges_and_non_ascii_lengths_do_not_expand_authority() {
        assert_eq!(stock_identity(i64::MIN), None);
        assert_eq!(stock_identity(i64::MAX), None);
        assert_eq!(selected_perk(&("é".repeat(48) + " sad")), Some("sad"));
        assert_eq!(selected_perk(&("é".repeat(49) + " sad")), None);
        assert_eq!(selected_perk("smiley\0"), None);
        assert!(!unlocked_perks(i64::MAX).contains(&"showscore"));
        assert!(!unlocked_perks(i64::MAX).contains(&"party"));
    }
}
