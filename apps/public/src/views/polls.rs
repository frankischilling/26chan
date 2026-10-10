//! Production poll views shared with the source-template visual fixtures.
use askama::Template;
use board_store::{PollSnapshot, PollSummary};

#[derive(Template)]
#[template(path = "polls.html")]
pub struct CataloguePage {
    pub polls: Vec<PollSummary>,
    pub copyright_year: i32,
}

#[derive(Template)]
#[template(path = "poll-options.html")]
pub struct OptionsPage {
    pub poll: PollSnapshot,
    pub form_token: String,
    pub copyright_year: i32,
}

struct ResultRow {
    caption: String,
    score: i64,
    percentage: String,
}

#[derive(Template)]
#[template(path = "poll-results.html")]
pub struct ResultsPage {
    id: i64,
    title: String,
    description: String,
    vote_count: i64,
    options: Vec<ResultRow>,
    copyright_year: i32,
}

impl ResultsPage {
    pub fn new(poll: PollSnapshot, copyright_year: i32) -> Self {
        let options = poll
            .options
            .into_iter()
            .map(|option| {
                let score = option.score.unwrap_or(0);
                ResultRow {
                    caption: option.caption,
                    score,
                    percentage: percentage(score, poll.vote_count),
                }
            })
            .collect();
        Self {
            id: poll.id,
            title: poll.title,
            description: poll.description,
            vote_count: poll.vote_count,
            options,
            copyright_year,
        }
    }
}

/// Presentation contract for stored counts in 0..=1_000_000_000: nearest
/// hundredth of a percent, positive half-ties rounded up, trailing zeros omitted.
/// Integer arithmetic avoids float-dependent output; this does not promise
/// arbitrary PHP numeric equivalence. A zero denominator displays zero percent.
fn percentage(score: i64, total: i64) -> String {
    if total == 0 {
        return "0".into();
    }
    let hundredths = (i128::from(score) * 10_000 + i128::from(total) / 2) / i128::from(total);
    let whole = hundredths / 100;
    match hundredths % 100 {
        0 => whole.to_string(),
        fraction if fraction % 10 == 0 => format!("{whole}.{}", fraction / 10),
        fraction => format!("{whole}.{fraction:02}"),
    }
}

#[cfg(test)]
mod tests {
    use super::percentage;

    #[test]
    fn bounded_percentages_round_and_omit_trailing_zeros() {
        for (score, total, expected) in [
            (0, 0, "0"),
            (1, 0, "0"),
            (0, 3, "0"),
            (1, 3, "33.33"),
            (2, 3, "66.67"),
            (1, 8, "12.5"),
            (1, 32, "3.13"),
            (1, 200, "0.5"),
            (1, 20_000, "0.01"),
            (1, 1_000_000_000, "0"),
            (1_000_000_000, 1_000_000_000, "100"),
            (999_999_999, 1_000_000_000, "100"),
            (1_000_000_000, 1, "100000000000"),
        ] {
            assert_eq!(percentage(score, total), expected);
        }
    }
}
