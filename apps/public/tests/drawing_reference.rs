#[test]
fn source_drawing_policy_is_imported_for_all_boards_without_claiming_replay_support() {
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/board-reference.json")).unwrap();
    let boards = reference["boards"].as_array().unwrap();
    assert_eq!(boards.len(), 82);
    for board in boards {
        let slug = board["slug"].as_str().unwrap();
        assert_eq!(board["oekaki"], matches!(slug, "i" | "qst" | "vip"));
        assert_eq!(board["oekaki_replays"], slug == "i");
        assert_eq!(board["oekaki_width"], 400);
        assert_eq!(board["oekaki_height"], 400);
        assert_eq!(
            board["oekaki"],
            board["source_policy"]["ENABLE_PAINTERJS"] == "yes"
        );
        assert_eq!(
            board["oekaki_replays"],
            board["source_policy"]["ENABLE_OEKAKI_REPLAYS"] == "yes"
        );
        assert_eq!(
            board["oekaki_width"].as_i64().unwrap().to_string(),
            board["source_policy"]["PAINTERJS_DIMS"].as_str().unwrap()
        );
    }
    let migration = include_str!("../../../migrations/0114_board_drawing_policy.sql");
    assert!(migration.contains("oekaki boolean NOT NULL DEFAULT false"));
    assert!(migration.contains("oekaki_replays boolean NOT NULL DEFAULT false"));
    assert!(migration.contains("oekaki_width integer NOT NULL DEFAULT 400"));
    assert!(migration.contains("oekaki_height integer NOT NULL DEFAULT 400"));
    assert!(migration.contains("('i',true,true,400,400)"));
    assert!(migration.contains("('qst',true,false,400,400)"));
    assert!(migration.contains("('vip',true,false,400,400)"));
    assert!(!migration.contains("BETWEEN 100 AND 800"));
    let original_import = include_str!("../../../migrations/0045_original_boards.sql");
    let columns = original_import
        .lines()
        .find(|line| line.starts_with("INSERT INTO"))
        .unwrap();
    assert!(!columns.contains("oekaki"));
}
