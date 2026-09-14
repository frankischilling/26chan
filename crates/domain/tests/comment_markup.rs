use board_domain::comment_markup::{MarkupPolicy, MarkupToken, Tag, parse_markup};
use proptest::prelude::*;

// Test-only projection. Production templates must escape Text themselves.
fn project(tokens: &[MarkupToken]) -> String {
    let mut output = String::new();
    for token in tokens {
        output.push_str(&match token {
            MarkupToken::Text(text) => board_domain::source_html_entities(text),
            MarkupToken::Break => "<br>".into(),
            MarkupToken::Open(Tag::Spoiler) => "<s>".into(),
            MarkupToken::Close(Tag::Spoiler) => "</s>".into(),
            MarkupToken::Open(Tag::Code) => "<pre class=\"prettyprint\">".into(),
            MarkupToken::Close(Tag::Code) => "</pre>".into(),
            MarkupToken::Open(Tag::Sjis) => "<span class=\"sjis\">".into(),
            MarkupToken::Close(Tag::Sjis) => "</span>".into(),
            MarkupToken::Open(Tag::Bold) => "<span class=\"mu-s\">".into(),
            MarkupToken::Close(Tag::Bold) => "</span>".into(),
            MarkupToken::Open(Tag::Italic) => "<span class=\"mu-i\">".into(),
            MarkupToken::Close(Tag::Italic) => "</span>".into(),
            MarkupToken::Open(Tag::Red) => "<span class=\"mu-r\">".into(),
            MarkupToken::Close(Tag::Red) => "</span>".into(),
            MarkupToken::Open(Tag::Green) => "<span class=\"mu-g\">".into(),
            MarkupToken::Close(Tag::Green) => "</span>".into(),
            MarkupToken::Open(Tag::Blue) => "<span class=\"mu-b\">".into(),
            MarkupToken::Close(Tag::Blue) => "</span>".into(),
        });
    }
    output
}

#[test]
fn source_op_passes_are_ordered_case_sensitive_and_limit_each_kind_to_one_level() {
    let policy = MarkupPolicy {
        op: true,
        code: true,
        ..Default::default()
    };
    for (raw, expected) in [
        (
            "[b]one[b]two[/b]three[/b]",
            "<span class=\"mu-s\">onetwothree</span>",
        ),
        (
            "[b]one[/b][b]two[/b]",
            "<span class=\"mu-s\">one</span><span class=\"mu-s\">two</span>",
        ),
        (
            "[i]<script>\nend",
            "<span class=\"mu-i\">&lt;script&gt;<br>end</span>",
        ),
        (
            "[red]r[/red][green]g[/green][blue]b[/blue]",
            "<span class=\"mu-r\">r</span><span class=\"mu-g\">g</span><span class=\"mu-b\">b</span>",
        ),
        ("[/b]orphan[B]upper[/B]", "[/b]orphan[B]upper[/B]"),
        (
            "[red][b]x[/red]y[/b]",
            "<span class=\"mu-r\"><span class=\"mu-s\">x</span>y</span>",
        ),
        ("[code][b][/code]", "<span class=\"mu-s\"></span>"),
        (
            "[code][b]x[/b][/code]",
            "<pre class=\"prettyprint\"><span class=\"mu-s\">x</span></pre>",
        ),
    ] {
        assert_eq!(project(&parse_markup(raw, policy)), expected, "{raw}");
    }
    let raw = "[b]x[/b][i]y[/i][red]r[/red][green]g[/green][blue]b[/blue]";
    assert_eq!(project(&parse_markup(raw, MarkupPolicy::default())), raw);
}

#[test]
fn source_spoilers_cross_lines_balance_and_limit_emitted_nesting() {
    let policy = MarkupPolicy {
        spoilers: true,
        ..Default::default()
    };
    for (raw, expected) in [
        ("[spoiler]first\nsecond[/spoiler]", "<s>first<br>second</s>"),
        (
            "[/spoiler]before[spoiler]tail",
            "[/spoiler]before<s>tail</s>",
        ),
        ("[spoiler]a[/spoiler]b[/spoiler]c", "<s>a</s>bc"),
        (
            "[spoiler]a[spoiler]b[spoiler]c[/spoiler]d[/spoiler]e[/spoiler]",
            "<s>a<s>bcd</s>e</s>",
        ),
        ("[spoiler]a[spoiler]b[spoiler]c", "<s>a<s>bc</s></s>"),
        ("[SPOILER]visible[/SPOILER]", "[SPOILER]visible[/SPOILER]"),
        ("[spoiler]x[/spoiler][spoiler]y", "<s>x</s><s>y</s>"),
        (
            "[spoiler]>>1 <script>[/spoiler]",
            "<s>&gt;&gt;1 &lt;script&gt;</s>",
        ),
    ] {
        assert_eq!(project(&parse_markup(raw, policy)), expected, "{raw}");
    }
}

#[test]
fn recursively_empty_spoilers_use_source_ascii_whitespace_and_real_breaks() {
    let policy = MarkupPolicy {
        spoilers: true,
        ..Default::default()
    };
    for (raw, expected) in [
        ("a[spoiler] \t\n[spoiler]\n[/spoiler]\n[/spoiler]b", "ab"),
        ("[spoiler]\n", ""),
        ("[spoiler][spoiler]x[/spoiler][/spoiler]", "<s><s>x</s></s>"),
        ("[spoiler]\u{a0}[/spoiler]", "<s>\u{a0}</s>"),
        ("[spoiler]\u{3000}[/spoiler]", "<s>\u{3000}</s>"),
        ("[spoiler]<br>[/spoiler]", "<s>&lt;br&gt;</s>"),
        ("[spoiler]&nbsp;[/spoiler]", "<s>&amp;nbsp;</s>"),
        ("[spoiler][code][/code][/spoiler]", "<s>[code][/code]</s>"),
    ] {
        assert_eq!(project(&parse_markup(raw, policy)), expected, "{raw}");
    }
}

#[test]
fn sjis_retains_the_source_spoiler_rollback_and_text_omission() {
    let policy = MarkupPolicy {
        sjis: true,
        ..Default::default()
    };
    for (raw, expected) in [
        ("[sjis]a\nb[/sjis]", "<span class=\"sjis\">a<br>b</span>"),
        (
            "[sjis]a[sjis]b[/sjis]c[/sjis]",
            "<span class=\"sjis\">abc</span>",
        ),
        (
            "[sjis]a[spoiler]b[/spoiler][/sjis]",
            "[sjis]a[spoiler]b[/spoiler][/sjis]",
        ),
        ("[sjis]a[spoiler]tail", "[sjis]a[spoiler]tail"),
        (
            "[spoiler]outside[/spoiler][sjis]omitted[/sjis]tail",
            "[spoiler]outside[/spoiler]<span class=\"sjis\"></span>tail",
        ),
        (
            "[spoiler]x[/spoiler][sjis]a[sjis]b[/sjis]c[/sjis]tail",
            "[spoiler]x[/spoiler]<span class=\"sjis\">a</span>tail",
        ),
        (
            "[spoiler]x[/spoiler][sjis]tail",
            "[spoiler]x[/spoiler]<span class=\"sjis\">tail</span>",
        ),
        (
            "[sjis]a[/spoiler][/sjis]",
            "<span class=\"sjis\">a[/spoiler]</span>",
        ),
        (
            "[/sjis]before[sjis]a[/sjis]outside[/sjis]tail",
            "[/sjis]before<span class=\"sjis\">a</span>outsidetail",
        ),
    ] {
        assert_eq!(project(&parse_markup(raw, policy)), expected, "{raw}");
    }
}

#[test]
fn short_code_counts_source_escaped_utf8_bytes_before_whole_comment_parsing() {
    let policy = MarkupPolicy {
        code: true,
        ..Default::default()
    };
    for raw in [
        "", "abcdef", "ééé", "日本", "&a", "'", "\"", "\nx", "[code]",
    ] {
        let source = format!("[code]{raw}[/code]");
        // A nested opening marker remains after the short-code replacement,
        // then receives an automatic closing marker in the full code pass.
        let expected = if raw == "[code]" {
            "<pre class=\"prettyprint\"></pre>".into()
        } else {
            board_domain::source_html_entities(raw).replace('\n', "<br>")
        };
        assert_eq!(
            project(&parse_markup(&source, policy)),
            expected,
            "{source}"
        );
    }
    for raw in ["abcdefg", "éééa", "日本a", "&ab", "'a", "\"a", "\nabc"] {
        let expected = format!(
            "<pre class=\"prettyprint\">{}</pre>",
            board_domain::source_html_entities(raw.strip_prefix('\n').unwrap_or(raw))
        );
        assert_eq!(
            project(&parse_markup(&format!("[code]{raw}[/code]"), policy)),
            expected,
            "{raw}"
        );
    }
    assert_eq!(
        project(&parse_markup("[code]a[/code][code]1234567[/code]", policy)),
        "a<pre class=\"prettyprint\">1234567</pre>"
    );
}

#[test]
fn ordered_passes_preserve_crossing_boundaries_and_code_break_cleanup() {
    let all = MarkupPolicy {
        code: true,
        sjis: true,
        spoilers: true,
        op: false,
    };
    for (raw, expected) in [
        ("[spoiler][code][/code][/spoiler]", "<s></s>"),
        ("[code][spoiler][/spoiler][/code]", ""),
        (
            "[code][spoiler]x[/spoiler][/code]",
            "<pre class=\"prettyprint\"><s>x</s></pre>",
        ),
        (
            "[spoiler][code]1234567[/spoiler][/code]",
            "<s><pre class=\"prettyprint\">1234567</s></pre>",
        ),
        (
            "[sjis][spoiler]text[/spoiler][/sjis]",
            "[sjis]<s>text</s>[/sjis]",
        ),
        (
            "[spoiler]x[/spoiler][sjis]omitted[/sjis]",
            "<s>x</s><span class=\"sjis\"></span>",
        ),
        (
            "a\n\n\n\nb[code]\n\n1234567[/code]",
            "a<br><br><br>b<pre class=\"prettyprint\"><br>1234567</pre>",
        ),
    ] {
        assert_eq!(project(&parse_markup(raw, all)), expected, "{raw}");
    }
    let raw = "[spoiler]x[/spoiler]\n[code]long code[/code]\n[sjis]art[/sjis]";
    assert_eq!(
        project(&parse_markup(raw, MarkupPolicy::default())),
        raw.replace('\n', "<br>")
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn arbitrary_unicode_and_markers_are_bounded_with_finite_balanced_tag_kinds(
        pieces in prop::collection::vec(prop_oneof![
            ".{0,40}", Just("[spoiler]".into()), Just("[/spoiler]".into()),
            Just("[sjis]".into()), Just("[/sjis]".into()),
            Just("[code]".into()), Just("[/code]".into()), Just("\n".into()),
            Just("[b]".into()), Just("[/b]".into()), Just("[i]".into()), Just("[/i]".into()),
            Just("[red]".into()), Just("[/red]".into()), Just("[green]".into()), Just("[/green]".into()),
            Just("[blue]".into()), Just("[/blue]".into())
        ], 0..800), spoilers in any::<bool>(), code in any::<bool>(), sjis in any::<bool>(), op in any::<bool>(),
    ) {
        let input = pieces.concat();
        let tokens = parse_markup(&input, MarkupPolicy { spoilers, code, sjis, op });
        prop_assert!(tokens.len() <= board_domain::MAX_COMMENT_CHARS + 16);
        let mut depths = [0usize; 8];
        for token in tokens {
            let (tag, opening) = match token {
                MarkupToken::Open(tag) => (tag, true),
                MarkupToken::Close(tag) => (tag, false),
                _ => continue,
            };
            let (index, enabled, limit) = match tag {
                Tag::Spoiler => (0, spoilers, 2), Tag::Code => (1, code, 2), Tag::Sjis => (2, sjis, 1),
                Tag::Bold => (3, op, 1), Tag::Italic => (4, op, 1), Tag::Red => (5, op, 1),
                Tag::Green => (6, op, 1), Tag::Blue => (7, op, 1),
            };
            prop_assert!(enabled);
            if opening { depths[index] += 1; prop_assert!(depths[index] <= limit); }
            else { prop_assert!(depths[index] > 0); depths[index] -= 1; }
        }
        prop_assert_eq!(depths, [0; 8]);
    }
}

#[test]
fn independent_input_cap_preserves_multibyte_boundaries_and_closes_open_tags() {
    let policy = MarkupPolicy {
        spoilers: true,
        ..Default::default()
    };
    let raw = format!("[spoiler]{}", "界".repeat(16_001));
    let result = parse_markup(&raw, policy);
    assert_eq!(
        result,
        vec![
            MarkupToken::Open(Tag::Spoiler),
            MarkupToken::Text("界".repeat(15_991)),
            MarkupToken::Close(Tag::Spoiler)
        ]
    );
    let raw = "[spoiler]".repeat(1700);
    assert!(project(&parse_markup(&raw, policy)).is_empty());
}
