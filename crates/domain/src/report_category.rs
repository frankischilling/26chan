//! Pure category selection from `4chan-old/modes/report.php:307–395`.
//!
//! No production category rows are supplied here. Callers provide rows already
//! ordered as the source's `ORDER BY board ASC`: its collation and ordering of
//! equal board values are unknown. We preserve that order within the scoped and
//! global groups rather than inventing an ID or title tie-breaker.
//! PHP's falsey "0" board string also enters the bottom/global group after its
//! exact-board eligibility check; it does not become globally eligible.
//!
//! These types describe decoded configuration, not an authoritative legacy SQL
//! schema. Target eligibility, reporter admission, HTTP decoding, persistence,
//! and effective report weights remain the caller's responsibility.

use crate::BoardSlug;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CategoryId(i64);

impl CategoryId {
    /// Positive signed-64-bit IDs are this module's representation choice.
    pub fn new(value: i64) -> Option<Self> {
        (value > 0).then_some(Self(value))
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

const ILLEGAL_ID: CategoryId = CategoryId(31);

#[derive(Clone, Copy, Debug)]
pub struct Category<'a> {
    pub id: CategoryId,
    /// SQL NULL is distinct from the empty-string global scope.
    pub board: Option<&'a str>,
    pub op_only: bool,
    pub reply_only: bool,
    pub image_only: bool,
    /// Exact comma-delimited board names, without trimming or case folding.
    pub exclude_boards: Option<&'a str>,
    pub title: &'a str,
    /// Configured base weight only; selection never computes effective weight.
    pub weight: f64,
    /// Raw configured filtering threshold, not a reporter filtering decision.
    pub filtered: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct Target<'a> {
    pub board: &'a BoardSlug,
    pub is_worksafe: bool,
    pub resto: u64,
    pub fsize: u64,
    pub filedeleted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogError {
    DuplicateId(CategoryId),
    NonfiniteWeight(CategoryId),
}

/// A borrowed, validated catalog. An empty catalog is valid and selects nothing.
#[derive(Debug)]
pub struct Catalog<'rows, 'text> {
    rows: &'rows [Category<'text>],
}

impl<'rows, 'text> Catalog<'rows, 'text> {
    /// Reject ambiguous IDs and nonfinite weights as configuration errors.
    /// These are explicit rewrite validation rules, not inferred PHP behavior.
    /// No finite weight bound or filtering threshold bound is invented here.
    pub fn new(rows: &'rows [Category<'text>]) -> Result<Self, CatalogError> {
        let mut ids = HashSet::with_capacity(rows.len());
        for category in rows {
            if !ids.insert(category.id) {
                return Err(CatalogError::DuplicateId(category.id));
            }
            if !category.weight.is_finite() {
                return Err(CatalogError::NonfiniteWeight(category.id));
            }
        }
        Ok(Self { rows })
    }

    pub fn select(&self, target: Target<'_>) -> Eligible<'rows, 'text> {
        let mut scoped = Vec::new();
        let mut global = Vec::new();
        let mut illegal = None;
        let board = target.board.as_str();
        let is_op = target.resto == 0;
        let has_image = target.fsize != 0 && !target.filedeleted;

        for category in self.rows {
            // The source extracts ID 31 before *all* scope restrictions.
            if category.id == ILLEGAL_ID {
                illegal = Some(category);
                continue;
            }
            let Some(scope) = category.board else {
                // PHP's strict comparison against '' does not treat NULL as
                // global; neither does its strict board comparison.
                continue;
            };
            let matches = match scope {
                "" => true,
                "_ws_" => target.is_worksafe,
                "_nws_" => !target.is_worksafe,
                specific => specific == board,
            };
            if !matches
                || (category.op_only && !is_op)
                || (category.reply_only && is_op)
                || (category.image_only && !has_image)
                || category.exclude_boards.is_some_and(|excluded| {
                    php_string_truthy(excluded) && excluded.split(',').any(|item| item == board)
                })
            {
                continue;
            }
            if !php_string_truthy(scope) {
                global.push(category);
            } else {
                scoped.push(category);
            }
        }
        scoped.extend(global);
        Eligible {
            rules: scoped,
            illegal,
        }
    }
}

#[derive(Debug)]
pub struct Eligible<'rows, 'text> {
    rules: Vec<&'rows Category<'text>>,
    illegal: Option<&'rows Category<'text>>,
}

impl<'rows, 'text> Eligible<'rows, 'text> {
    pub fn rules(&self) -> &[&'rows Category<'text>] {
        &self.rules
    }

    pub fn illegal(&self) -> Option<&'rows Category<'text>> {
        self.illegal
    }

    /// Checks membership only, never post visibility or reporting authority.
    pub fn validate(&self, id: CategoryId) -> Result<Selection<'rows, 'text>, Rejection> {
        if let Some(category) = self.illegal.filter(|category| category.id == id) {
            return Ok(Selection {
                category,
                kind: Kind::Illegal,
            });
        }
        self.rules
            .iter()
            .copied()
            .find(|category| category.id == id)
            .map(|category| Selection {
                category,
                kind: Kind::Rule,
            })
            .ok_or(Rejection::InvalidCategory)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rule,
    Illegal,
}

#[derive(Clone, Copy, Debug)]
pub struct Selection<'rows, 'text> {
    pub category: &'rows Category<'text>,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    InvalidCategory,
}

impl Rejection {
    pub fn message(self) -> &'static str {
        "Invalid category selected."
    }
}

/// Select the raw scalar field using `imgboard.php:7407–7423` precedence.
/// Only absent, empty, and exactly "0" are falsey PHP strings. A truthy but
/// malformed `cat` wins over `cat_id`; a later parser must not fall back.
///
/// Numeric conversion is deliberately NOT implemented: PHP integer coercion
/// and strict canonical decimal parsing are different compatibility policies.
/// The HTTP adapter must separately bound/decode fields and reject ambiguous
/// duplicate or array-shaped parameters before calling this function.
pub fn selected_field<'a>(
    cat: Option<&'a str>,
    cat_id: Option<&'a str>,
) -> Result<&'a str, Rejection> {
    let truthy = |value: &&str| php_string_truthy(value);
    cat.filter(truthy)
        .or_else(|| cat_id.filter(truthy))
        .ok_or(Rejection::InvalidCategory)
}

fn php_string_truthy(value: &str) -> bool {
    !value.is_empty() && value != "0"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: i64) -> CategoryId {
        CategoryId::new(value).unwrap()
    }

    fn category(value: i64, board: Option<&str>) -> Category<'_> {
        Category {
            id: id(value),
            board,
            op_only: false,
            reply_only: false,
            image_only: false,
            exclude_boards: None,
            title: "Synthetic category fixture",
            weight: 1.0,
            filtered: 0,
        }
    }

    fn target(board: &BoardSlug) -> Target<'_> {
        Target {
            board,
            is_worksafe: true,
            resto: 0,
            fsize: 0,
            filedeleted: false,
        }
    }

    fn ids(eligible: &Eligible<'_, '_>) -> Vec<i64> {
        eligible.rules().iter().map(|row| row.id.get()).collect()
    }

    #[test]
    fn scope_null_empty_groups_and_input_order_are_distinct() {
        let board = BoardSlug::parse("fixture").unwrap();
        let rows = [
            category(99, None),
            category(80, Some("")),
            category(2, Some("")),
            category(7, Some("_nws_")),
            category(90, Some("_ws_")),
            category(3, Some("_ws_")),
            category(60, Some("fixture")),
            category(5, Some("fixture")),
            category(4, Some("other")),
        ];
        let catalog = Catalog::new(&rows).unwrap();
        let mut input = target(&board);
        assert_eq!(ids(&catalog.select(input)), [90, 3, 60, 5, 80, 2]);
        input.is_worksafe = false;
        assert_eq!(ids(&catalog.select(input)), [7, 60, 5, 80, 2]);
    }

    #[test]
    fn exclusions_are_exact_untrimmed_comma_delimited_names() {
        let board = BoardSlug::parse("fixture").unwrap();
        for (excluded, included) in [
            (None, true),
            (Some(""), true),
            (Some("fixture"), false),
            (Some("before,fixture,after"), false),
            (Some(",fixture,"), false),
            (Some("fixtures"), true),
            (Some(" fixture"), true),
            (Some("fixture "), true),
            (Some("FIXTURE"), true),
        ] {
            let rows = [Category {
                exclude_boards: excluded,
                ..category(1, Some(""))
            }];
            assert_eq!(
                Catalog::new(&rows)
                    .unwrap()
                    .select(target(&board))
                    .rules()
                    .len(),
                usize::from(included),
                "{excluded:?}"
            );
        }
    }

    #[test]
    fn zero_board_retains_php_falsey_scope_group_and_exclusion_behavior() {
        let board = BoardSlug::parse("0").unwrap();
        let rows = [
            category(1, Some("")),
            Category {
                exclude_boards: Some("0"),
                ..category(2, Some("0"))
            },
            category(3, Some("_ws_")),
            Category {
                exclude_boards: Some(",0,"),
                ..category(4, Some(""))
            },
        ];
        let catalog = Catalog::new(&rows).unwrap();
        assert_eq!(ids(&catalog.select(target(&board))), [3, 1, 2]);
        let different_board = BoardSlug::parse("fixture").unwrap();
        assert_eq!(ids(&catalog.select(target(&different_board))), [3, 1, 4]);
    }

    #[test]
    fn post_kind_and_image_checks_follow_source_fields() {
        let board = BoardSlug::parse("fixture").unwrap();
        let rows = [
            Category {
                op_only: true,
                ..category(1, Some(""))
            },
            Category {
                reply_only: true,
                ..category(2, Some(""))
            },
            Category {
                op_only: true,
                reply_only: true,
                ..category(3, Some(""))
            },
            Category {
                image_only: true,
                ..category(4, Some(""))
            },
        ];
        let catalog = Catalog::new(&rows).unwrap();
        for resto in [0, 17] {
            for fsize in [0, 1, u64::MAX] {
                for filedeleted in [false, true] {
                    let input = Target {
                        resto,
                        fsize,
                        filedeleted,
                        ..target(&board)
                    };
                    let mut expected = vec![if resto == 0 { 1 } else { 2 }];
                    if fsize != 0 && !filedeleted {
                        expected.push(4);
                    }
                    assert_eq!(ids(&catalog.select(input)), expected);
                }
            }
        }
    }

    #[test]
    fn illegal_id_bypasses_every_scope_filter_but_needs_a_catalog_row() {
        let board = BoardSlug::parse("fixture").unwrap();
        for scope in [None, Some("other"), Some("_nws_")] {
            let rows = [Category {
                op_only: true,
                reply_only: true,
                image_only: true,
                exclude_boards: Some("fixture"),
                ..category(31, scope)
            }];
            let catalog = Catalog::new(&rows).unwrap();
            let eligible = catalog.select(target(&board));
            assert!(eligible.rules().is_empty());
            assert_eq!(eligible.illegal().unwrap().id, id(31));
            assert_eq!(eligible.validate(id(31)).unwrap().kind, Kind::Illegal);
        }
        let rows = [category(1, Some("")), category(2, Some("other"))];
        let catalog = Catalog::new(&rows).unwrap();
        let eligible = catalog.select(target(&board));
        assert!(eligible.illegal().is_none());
        assert_eq!(eligible.validate(id(1)).unwrap().kind, Kind::Rule);
        for missing in [2, 31, 12345] {
            assert_eq!(
                eligible.validate(id(missing)).unwrap_err(),
                Rejection::InvalidCategory
            );
        }
        let empty = Catalog::new(&[]).unwrap();
        assert!(empty.select(target(&board)).rules().is_empty());
        assert!(empty.select(target(&board)).illegal().is_none());
    }

    #[test]
    fn catalog_rejects_duplicate_ids_and_nonfinite_weights_without_inventing_bounds() {
        for value in [1, 31] {
            let rows = [category(value, None), category(value, Some("fixture"))];
            assert_eq!(
                Catalog::new(&rows).unwrap_err(),
                CatalogError::DuplicateId(id(value))
            );
        }
        for weight in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let rows = [Category {
                weight,
                ..category(1, Some(""))
            }];
            assert_eq!(
                Catalog::new(&rows).unwrap_err(),
                CatalogError::NonfiniteWeight(id(1))
            );
        }
        let board = BoardSlug::parse("fixture").unwrap();
        for weight in [-1.0, -0.0, 0.0, 0.125, f64::MAX] {
            let rows = [Category {
                weight,
                filtered: -7,
                ..category(1, Some(""))
            }];
            let catalog = Catalog::new(&rows).unwrap();
            let eligible = catalog.select(target(&board));
            let selected = eligible.validate(id(1)).unwrap();
            assert_eq!(selected.category.weight.to_bits(), weight.to_bits());
            assert_eq!(selected.category.filtered, -7);
            assert_eq!(selected.category.title, "Synthetic category fixture");
        }
        for invalid in [i64::MIN, -1, 0] {
            assert!(CategoryId::new(invalid).is_none());
        }
        assert_eq!(CategoryId::new(i64::MAX).unwrap().get(), i64::MAX);
    }

    #[test]
    fn raw_field_precedence_uses_php_string_falseyness_without_numeric_parsing() {
        for cat in [None, Some(""), Some("0")] {
            for cat_id in [None, Some(""), Some("0")] {
                assert_eq!(selected_field(cat, cat_id), Err(Rejection::InvalidCategory));
            }
            assert_eq!(selected_field(cat, Some("17")), Ok("17"));
        }
        for cat in [
            "31", "garbage", " ", "00", "0.0", "-1", "+31", "031", "31suffix",
        ] {
            assert_eq!(selected_field(Some(cat), Some("17")), Ok(cat));
            assert_eq!(selected_field(None, Some(cat)), Ok(cat));
        }
        assert_eq!(
            Rejection::InvalidCategory.message(),
            "Invalid category selected."
        );
    }
}
