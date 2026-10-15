//! Boards with a captured source still-image processing policy.
/// Custom development boards retain the legacy image path until configured.
pub fn configured(board: &str) -> bool {
    matches!(
        board,
        "3" | "a"
            | "aco"
            | "adv"
            | "an"
            | "asp"
            | "b"
            | "bant"
            | "biz"
            | "c"
            | "cgl"
            | "ck"
            | "cm"
            | "co"
            | "d"
            | "diy"
            | "e"
            | "f"
            | "fa"
            | "fit"
            | "g"
            | "gd"
            | "gif"
            | "h"
            | "hc"
            | "his"
            | "hm"
            | "hr"
            | "i"
            | "ic"
            | "int"
            | "j"
            | "jp"
            | "k"
            | "lgbt"
            | "lit"
            | "m"
            | "mlp"
            | "mu"
            | "n"
            | "news"
            | "o"
            | "out"
            | "p"
            | "po"
            | "pol"
            | "pw"
            | "qa"
            | "qb"
            | "qst"
            | "r"
            | "r9k"
            | "s"
            | "s4s"
            | "sci"
            | "soc"
            | "sp"
            | "t"
            | "test"
            | "tg"
            | "toy"
            | "trash"
            | "trv"
            | "tv"
            | "u"
            | "v"
            | "vg"
            | "vip"
            | "vm"
            | "vmg"
            | "vp"
            | "vr"
            | "vrpg"
            | "vst"
            | "vt"
            | "w"
            | "wg"
            | "wsg"
            | "wsr"
            | "x"
            | "xs"
            | "y"
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn configured_boards_match_the_pinned_source_presentation_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/board-reference.json")).unwrap();
        let boards = fixture["source_image_presentation"].as_object().unwrap();
        assert_eq!(boards.len(), 82);
        for board in boards.keys() {
            assert!(super::configured(board));
        }
        for board in ["", "/p/", "P", "custom-fixture"] {
            assert!(!super::configured(board));
        }
    }
}
