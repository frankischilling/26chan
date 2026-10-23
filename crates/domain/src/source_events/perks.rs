//! The score helper's active Unicode perks, including repeated pool entries.
//! Historical image/animation cases are not made eligible by this module.
#[rustfmt::skip]
mod data;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Perk {
    name: &'static str,
    entities: &'static str,
    stored_id: i16,
}
impl Perk {
    /// Stable one-based index in the pinned source pools, including duplicates.
    pub fn stored_id(self) -> i16 {
        self.stored_id
    }
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
    let mut offset = 0;
    for (key, values) in data::POOLS {
        if *key == name {
            let entities = values.get(index)?;
            return Some(Perk {
                name: key,
                entities,
                stored_id: (offset + index + 1) as i16,
            });
        }
        offset += values.len();
    }
    None
}
/// A stored choice grants only a finite Unicode glyph, never caller HTML.
pub fn from_stored(id: i16) -> Option<Perk> {
    let mut index = usize::try_from(id.checked_sub(1)?).ok()?;
    for (name, values) in data::POOLS {
        if index < values.len() {
            return Some(Perk {
                name,
                entities: values[index],
                stored_id: id,
            });
        }
        index -= values.len();
    }
    None
}

/// Choose uniformly from the original pool. Repeated entries keep their source
/// weight. The caller must hold the active event gate before saving the result.
pub fn generate(
    options: &str,
) -> Result<Option<Perk>, crate::posting_randomizers::GenerationError> {
    use ring::rand::{SecureRandom, SystemRandom};
    let Some(name) = super::selected_perk(options) else {
        return Ok(None);
    };
    let count = choices(name, 0) as u32;
    if count == 0 {
        return Err(crate::posting_randomizers::GenerationError::InvalidRequest);
    }
    let ceiling = u32::MAX - u32::MAX % count;
    for _ in 0..16 {
        let mut bytes = [0; 4];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| crate::posting_randomizers::GenerationError::RandomnessUnavailable)?;
        let value = u32::from_be_bytes(bytes);
        if value < ceiling {
            return Ok(choose(name, 0, (value % count) as usize));
        }
    }
    Err(crate::posting_randomizers::GenerationError::RandomnessUnavailable)
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
    fn persisted_choices_round_trip_without_accepting_arbitrary_markup() {
        let mut next = 1;
        for name in super::super::PERKS {
            for index in 0..choices(name, 0) {
                let perk = choose(name, 0, index).unwrap();
                assert_eq!(perk.stored_id(), next);
                assert_eq!(from_stored(next), Some(perk));
                next += 1;
            }
        }
        assert_eq!(next, 232);
        for invalid in [i16::MIN, -1, 0, 232, i16::MAX] {
            assert!(from_stored(invalid).is_none());
        }
        for _ in 0..64 {
            let perk = generate("unknown animal food").unwrap().unwrap();
            assert_eq!(perk.name(), "animal");
            assert_eq!(from_stored(perk.stored_id()), Some(perk));
        }
        assert_eq!(generate("spooky showscore <img>").unwrap(), None);
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
