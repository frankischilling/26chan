/// The HTML catalog checks sticky/permaage but does not exclude undead.
/// Zero is an already-reached limit, not an unlimited-image setting.
pub fn catalog_limited(sticky: bool, permaage: bool, images: u64, limit: u32) -> bool {
    !sticky && !permaage && images >= u64::from(limit)
}

/// All public JSON representations use the source's cached thread flags,
/// including catalog.json. The HTML catalog has its own rule above.
pub fn json_limited(sticky: bool, permaage: bool, undead: bool, images: u64, limit: u32) -> bool {
    !undead && catalog_limited(sticky, permaage, images, limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_image_indicators_keep_the_catalog_and_json_branches_distinct() {
        for sticky in [false, true] {
            for permaage in [false, true] {
                for undead in [false, true] {
                    for images in [0, 1, 2, u64::MAX] {
                        for limit in [0, 1, 2, u32::MAX] {
                            let reached = images >= u64::from(limit);
                            assert_eq!(
                                catalog_limited(sticky, permaage, images, limit),
                                reached && !sticky && !permaage
                            );
                            assert_eq!(
                                json_limited(sticky, permaage, undead, images, limit),
                                reached && !sticky && !permaage && !undead
                            );
                        }
                    }
                }
            }
        }
        assert!(catalog_limited(false, false, 0, 0));
        assert!(json_limited(false, false, false, 0, 0));
        assert!(!json_limited(false, false, true, 7, 7));
        assert!(catalog_limited(false, false, 7, 7));
    }
}
