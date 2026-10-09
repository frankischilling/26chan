//! Source navigation is a presentation projection of an already-authorized snapshot.
//! It deliberately does not replace board titles or public discovery metadata.
use board_store::Board;
#[path = "navigation_data.rs"]
mod data;

#[derive(Clone, Copy)]
pub enum Destination {
    Index,
    Catalog,
    Archive,
}

pub struct Link<'a> {
    pub slug: &'a str,
    pub title: &'a str,
    pub nws: bool,
    pub href: String,
}

impl Destination {
    fn href(self, slug: &str) -> String {
        let suffix = match self {
            Self::Catalog if slug != "f" => "catalog",
            Self::Archive if slug != "f" && slug != "b" => "archive",
            _ => "",
        };
        format!("/{slug}/{suffix}")
    }
}

/// Never add a missing or private source entry, even if the current board is it.
pub fn groups(boards: &[Board], destination: Destination) -> Vec<Vec<Link<'_>>> {
    data::HEADER_GROUPS
        .iter()
        .map(|group| {
            group
                .iter()
                .filter_map(|&(slug, title)| {
                    let board = boards
                        .iter()
                        .find(|board| board.slug == slug && !board.staff_only)?;
                    Some(Link {
                        slug: &board.slug,
                        title,
                        nws: data::HEADER_NWS_SLUGS.contains(&slug),
                        href: destination.href(slug),
                    })
                })
                .collect::<Vec<_>>()
        })
        .filter(|group| !group.is_empty())
        .collect()
}

pub fn mobile(boards: &[Board]) -> Vec<Link<'_>> {
    let mut links: Vec<_> = groups(boards, Destination::Index)
        .into_iter()
        .flatten()
        .collect();
    links.sort_by(|a, b| a.slug.cmp(b.slug));
    links
}

pub fn needs_fallback(boards: &[Board], current: &Board) -> bool {
    !current.staff_only && !mobile(boards).iter().any(|link| link.slug == current.slug)
}

pub struct DirectoryEntry<'a> {
    pub board: &'a Board,
    pub title: &'a str,
}

/// Keep discovery within the existing 100-row snapshot bound, using the static
/// directory's separate label/order
/// where known and the escaped operator title for additional boards.
pub fn directory(boards: &[Board]) -> Vec<DirectoryEntry<'_>> {
    let mut entries: Vec<_> = boards
        .iter()
        .take(100)
        .filter(|board| !board.staff_only)
        .collect();
    entries.sort_by_key(|board| {
        let source = data::DIRECTORY_LABELS
            .iter()
            .position(|&(slug, _)| slug == board.slug);
        (
            source.unwrap_or(usize::MAX),
            board.source_order,
            &board.slug,
        )
    });
    entries
        .into_iter()
        .map(|board| DirectoryEntry {
            title: data::DIRECTORY_LABELS
                .iter()
                .find(|&&(slug, _)| slug == board.slug)
                .map_or(board.title.as_str(), |&(_, title)| title),
            board,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use askama::Template;

    fn board(slug: &str) -> Board {
        let mut board = base();
        board.slug = slug.into();
        board.title = format!("Operator {slug} <script>& title");
        board
    }

    fn base() -> Board {
        Board {
            replies_shown: 5,
            source_order: 1000,
            catalog_enabled: true,
            json_enabled: true,
            staff_only: false,
            meta_board: false,
            upload_board: false,
            rss_enabled: true,
            slug: "test".into(),
            title: "Test".into(),
            description: String::new(),
            show_blotter: true,
            board_subtitle: board_store::BoardSubtitle::None,
            max_comment_chars: 16_000,
            max_authorized_comment_chars: 10000,
            comment_code_spacing: true,
            comment_sjis_spacing: false,
            math_tags: false,
            oekaki: false,
            oekaki_replays: false,
            oekaki_width: 400,
            oekaki_height: 400,
            comment_max_lines: 100,
            comment_spoiler_cleanup: true,
            custom_spoiler_count: 0,
            spoiler_thumbnail_assets: vec!["spoiler.png".into()],
            require_subject: false,
            op_markup: false,
            forced_anon: false,
            strip_tripcode: false,
            user_ids: false,
            country_flags: false,
            board_flags: vec![],
            board_flag_type: "pol".into(),
            text_only: false,
            reply_limit: 1000,
            bump_limit: 300,
            permasage_hours: 0,
            posting_reply_seconds: 0,
            posting_image_seconds: 0,
            posting_thread_seconds: 0,
            user_thread_limit: 5,
            user_thread_period_hours: 24,
            op_bump_limit: true,
            op_bump_initial_seconds: 900,
            op_bump_repeat_seconds: 300,
            thread_limit: 10,
            expire_neglected: true,
            threads_per_page: 10,
            worksafe: true,
            archive_retention_seconds: 0,
            archive_limit: 0,
            image_limit: 100,
            dice_roll: false,
            fortune_trip: false,
            robot9000: false,
            robot9000_state_limit: 100000,
            word_filter_enabled: false,
            word_filter_profile: 0,
        }
    }

    fn page(boards: Vec<Board>, current: Board) -> super::super::BoardPage {
        super::super::BoardPage {
            blotter: vec![],
            spoiler_thumbnail: String::new(),
            navigation_boards: boards,
            quote: String::new(),
            catalog_hidden: vec![],
            board: current,
            threads: vec![],
            page_number: 1,
            parent: 0,
            previous: String::new(),
            next: String::new(),
            catalog: false,
            catalog_options: crate::catalog::Options::default(),
            media_origin: String::new(),
        }
    }

    #[test]
    fn exact_source_groups_and_mobile_membership_ignore_operator_titles_and_order() {
        assert_eq!(
            data::HEADER_GROUPS
                .iter()
                .map(|g| g.len())
                .collect::<Vec<_>>(),
            [27, 2, 4, 4, 40]
        );
        let mut boards: Vec<_> = data::HEADER_GROUPS
            .iter()
            .flat_map(|group| group.iter())
            .map(|&(slug, _)| board(slug))
            .collect();
        boards.reverse();
        boards.extend([board("j"), board("custom"), board("asp")]);
        let groups = groups(&boards, Destination::Index);
        for (actual, expected) in groups.iter().zip(data::HEADER_GROUPS) {
            assert_eq!(
                actual.iter().map(|l| (l.slug, l.title)).collect::<Vec<_>>(),
                *expected
            );
        }
        let mobile = mobile(&boards);
        assert_eq!(mobile.len(), 77);
        assert!(mobile.windows(2).all(|p| p[0].slug < p[1].slug));
        assert!(
            !mobile
                .iter()
                .any(|l| ["j", "custom", "asp"].contains(&l.slug))
        );
        for (slug, label) in [
            ("p", "Photo"),
            ("diy", "Do It Yourself"),
            ("lgbt", "LGBT"),
            ("s4s", "Shit 4chan Says"),
        ] {
            assert_eq!(mobile.iter().find(|l| l.slug == slug).unwrap().title, label);
            let rendered = page(boards.clone(), board(slug)).render().unwrap();
            assert!(rendered.contains(&format!("title=\"{label}\"")));
            assert!(rendered.contains(&format!("Operator {slug} &#60;script&#62;&#38; title")));
            assert!(!rendered.contains("<script>& title"));
        }
    }

    #[test]
    fn partial_and_empty_snapshots_have_only_nonempty_source_groups() {
        let boards = vec![board("s4s"), board("custom"), board("f")];
        let groups = groups(&boards, Destination::Index);
        assert_eq!(
            groups
                .iter()
                .map(|group| group.iter().map(|link| link.slug).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            [vec!["f"], vec!["s4s"]]
        );
        assert!(super::groups(&[], Destination::Index).is_empty());
        assert!(super::groups(&[board("custom")], Destination::Index).is_empty());
        let html = page(boards, board("custom")).render().unwrap();
        assert_eq!(html.matches("data-public-board-group=").count(), 4);
        assert!(!html.contains(">[]</span>"));
        let html = page(vec![], board("custom")).render().unwrap();
        assert!(!html.contains("data-public-board-group="));
        assert!(html.contains("data-current-board-fallback=\"true\""));
    }

    #[test]
    fn destinations_preserve_source_exceptions_without_mutating_policy() {
        let mut boards = vec![board("a"), board("b"), board("f")];
        for board in &mut boards {
            board.catalog_enabled = false;
            board.archive_retention_seconds = 0;
        }
        for (destination, expected) in [
            (Destination::Index, ["/a/", "/b/", "/f/"]),
            (Destination::Catalog, ["/a/catalog", "/b/catalog", "/f/"]),
            (Destination::Archive, ["/a/archive", "/b/", "/f/"]),
        ] {
            assert_eq!(
                groups(&boards, destination)
                    .into_iter()
                    .flatten()
                    .map(|l| l.href)
                    .collect::<Vec<_>>(),
                expected
            );
        }
        assert!(
            boards
                .iter()
                .all(|b| !b.catalog_enabled && b.archive_retention_seconds == 0)
        );
    }

    #[test]
    fn actual_catalog_and_archive_templates_share_source_groups_and_mobile_index_values() {
        let boards = vec![board("a"), board("b"), board("f")];
        let mut catalog = page(boards.clone(), board("a"));
        catalog.catalog = true;
        let html = catalog.render().unwrap();
        assert_eq!(
            html.matches("href=\"/a/catalog\" title=\"Anime &#38; Manga\"")
                .count(),
            2
        );
        assert_eq!(
            html.matches("href=\"/b/catalog\" title=\"Random\"").count(),
            2
        );
        let archive = super::super::ArchivePage {
            navigation_boards: boards,
            board: board("a"),
            entries: vec![],
        };
        let html = archive.render().unwrap();
        assert_eq!(
            html.matches("href=\"/a/archive\" title=\"Anime &#38; Manga\"")
                .count(),
            2
        );
        assert_eq!(html.matches("href=\"/b/\" title=\"Random\"").count(), 2);
        assert_eq!(html.matches("href=\"/f/\" title=\"Flash\"").count(), 2);
        assert!(html.contains("<option value=\"a\" selected>/a/ - Anime &#38; Manga</option>"));
    }

    #[test]
    fn filtering_fallback_escaping_and_snapshot_reuse_are_separate_from_membership() {
        let mut private = board("b");
        private.staff_only = true;
        let mut boards = vec![board("a"), private, board("custom")];
        let current = board("custom");
        assert!(needs_fallback(&boards, &current));
        assert!(!needs_fallback(&boards, &boards[1]));
        let before = page(boards.clone(), current.clone()).render().unwrap();
        assert!(before.contains("data-current-board-fallback=\"true\" selected>/custom/ - Operator custom &#60;script&#62;&#38; title"));
        assert!(!before.contains("title=\"Random\""));
        assert!(!before.contains("href=\"/custom/\" title="));
        assert!(!before.contains("href=\"/j/\""));
        boards[0].worksafe = false;
        assert!(!boards[0].worksafe);
        assert!(
            groups(&boards, Destination::Index)
                .iter()
                .flatten()
                .all(|link| !link.nws)
        );
        let after = page(boards, current).render().unwrap();
        assert_eq!(
            before, after,
            "active header.txt wrappers are independent of board worksafe policy"
        );
        assert!(!after.contains("class=\"nwsb\"><a href=\"/a/\""));
    }

    #[test]
    fn directory_retains_full_authorized_discovery_and_uses_its_own_identity() {
        assert_eq!(data::DIRECTORY_LABELS.len(), 78);
        let mut boards: Vec<_> = data::DIRECTORY_LABELS
            .iter()
            .map(|&(slug, _)| board(slug))
            .collect();
        boards.reverse();
        boards.extend((0..20).map(|i| board(&format!("custom{i}"))));
        let mut private = board("private");
        private.staff_only = true;
        boards.push(private);
        let entries = directory(&boards);
        assert_eq!(entries.len(), 98);
        assert_eq!(
            entries[..78]
                .iter()
                .map(|e| (e.board.slug.as_str(), e.title))
                .collect::<Vec<_>>(),
            data::DIRECTORY_LABELS
        );
        let many: Vec<_> = (0..120).map(|i| board(&format!("owned{i}"))).collect();
        assert_eq!(directory(&many).len(), 100);
        let html = super::super::Home { boards }.render().unwrap();
        assert!(html.contains("/diy/ - Do-It-Yourself"));
        assert!(html.contains("/custom19/ - Operator custom19 &#60;script&#62;&#38; title"));
        assert!(!html.contains("/private/"));
        assert!(!html.contains("<script>& title"));
    }
}
