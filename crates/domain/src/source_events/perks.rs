//! The score helper's active Unicode perks, including repeated pool entries.
//! Historical image/animation cases are not made eligible by this module.
#[rustfmt::skip]
mod data;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Perk {
    name: &'static str,
    entities: &'static str,
}
impl Perk {
    pub fn name(self) -> &'static str {
        self.name
    }
    pub fn glyph(self) -> String {
        self.entities
            .split(';')
            .filter(|part| !part.is_empty())
            .map(|part| {
                char::from_u32(
                    u32::from_str_radix(part.strip_prefix("&#x").expect("release entity"), 16)
                        .expect("release scalar"),
                )
                .expect("release Unicode")
            })
            .collect()
    }
    /// Only release-owned numeric entities enter this fragment. A renderer may
    /// instead use `glyph` in its normal escaped text node.
    pub fn source_html(self) -> String {
        format!("<div class=\"like-perk-cnt\">{}</div>", self.entities)
    }
}
pub fn choices(name: &str, score: i64) -> usize {
    if !super::unlocked_perks(score).contains(&name) {
        return 0;
    }
    data::POOLS
        .iter()
        .find(|(key, _)| *key == name)
        .map_or(0, |(_, values)| values.len())
}
/// The caller supplies a server-generated index in the original pool, rather
/// than a visitor-specified winning glyph. Out-of-range indices are rejected.
pub fn choose(name: &str, score: i64, index: usize) -> Option<Perk> {
    if !super::unlocked_perks(score).contains(&name) {
        return None;
    }
    let (name, values) = data::POOLS.iter().find(|(key, _)| *key == name)?;
    values.get(index).map(|entities| Perk { name, entities })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_active_glyph_matches_the_executed_source_branch() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/event-perk-reference.json"
        ))
        .unwrap();
        assert_eq!(
            data::POOLS
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            super::super::PERKS
        );
        for row in fixture["cases"].as_array().unwrap() {
            let name = row["name"].as_str().unwrap();
            let index = row["index"].as_u64().unwrap() as usize;
            let perk = choose(name, 0, index).unwrap();
            assert_eq!(perk.name(), name);
            assert_eq!(perk.source_html(), row["html"]);
            assert_eq!(perk.glyph(), row["glyph"]);
            assert!(choose(name, -1, index).is_none());
        }
    }
    #[test]
    fn eligibility_never_accepts_markup_unknown_choices_or_old_commented_perks() {
        for name in [
            "<img>",
            "party",
            "showscore",
            "spooky",
            "dino",
            "marquee",
            "",
            "Smiley",
        ] {
            assert_eq!(choices(name, i64::MAX), 0);
            assert!(choose(name, i64::MAX, 0).is_none());
        }
        for name in super::super::PERKS {
            assert!(choices(name, 0) > 0);
            assert!(choose(name, 0, choices(name, 0)).is_none());
            assert!(choose(name, 0, usize::MAX).is_none());
        }
    }
}
