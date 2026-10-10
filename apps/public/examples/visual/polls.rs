//! Fixed synthetic data rendered by production views and independent PHP references.
use askama::Template;
use axum::{Router, http::header, response::Html, routing::get};
use board_public::views::polls::{CataloguePage, OptionsPage, ResultsPage};
use board_store::{PollOption, PollSnapshot, PollSummary};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    copyright_year: i32,
    token: String,
    catalogue: Vec<Summary>,
    poll: Poll,
}

#[derive(Deserialize)]
struct Summary {
    id: i64,
    title: String,
}

#[derive(Deserialize)]
struct Poll {
    id: i64,
    title: String,
    description: String,
    vote_count: i64,
    options: Vec<OptionRow>,
}

#[derive(Deserialize)]
struct OptionRow {
    id: i64,
    caption: String,
    score: Option<i64>,
}

fn page(name: &str) -> String {
    let data: Fixture =
        serde_json::from_str(include_str!("../../../../tests/fixtures/polls/data.json"))
            .expect("fixed poll visual data");
    if name.ends_with("catalogue") {
        return CataloguePage {
            polls: if name == "empty-catalogue" {
                vec![]
            } else {
                data.catalogue
                    .into_iter()
                    .map(|item| PollSummary {
                        id: item.id,
                        title: item.title,
                    })
                    .collect()
            },
            copyright_year: data.copyright_year,
        }
        .render()
        .expect("production poll catalogue");
    }
    let poll = PollSnapshot {
        id: data.poll.id,
        title: data.poll.title,
        description: if matches!(name, "options-no-description" | "empty-results") {
            String::new()
        } else {
            data.poll.description
        },
        vote_count: if name == "empty-results" {
            0
        } else {
            data.poll.vote_count
        },
        accepting_votes: true,
        options: if name == "empty-results" {
            vec![]
        } else {
            data.poll
                .options
                .into_iter()
                .map(|option| PollOption {
                    id: option.id,
                    caption: option.caption,
                    score: option.score,
                })
                .collect()
        },
    };
    if name.ends_with("results") {
        ResultsPage::new(poll, data.copyright_year)
            .render()
            .expect("production poll results")
    } else {
        OptionsPage {
            poll,
            form_token: data.token,
            copyright_year: data.copyright_year,
        }
        .render()
        .expect("production poll options")
    }
}

pub fn routes() -> Router {
    let mut router = Router::new().route(
        "/poll-visual/reference.css",
        get(|| async {
            (
                [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                include_str!("../../../../tests/fixtures/polls/source-polls.css")
                    .replace("//s.4cdn.org/image/fade.png", "/static/themes/fade.png"),
            )
        }),
    );
    for (name, reference) in [
        (
            "catalogue",
            include_str!("../../../../tests/fixtures/polls/catalogue.html"),
        ),
        (
            "empty-catalogue",
            include_str!("../../../../tests/fixtures/polls/empty-catalogue.html"),
        ),
        (
            "options",
            include_str!("../../../../tests/fixtures/polls/options.html"),
        ),
        (
            "options-no-description",
            include_str!("../../../../tests/fixtures/polls/options-no-description.html"),
        ),
        (
            "results",
            include_str!("../../../../tests/fixtures/polls/results.html"),
        ),
        (
            "empty-results",
            include_str!("../../../../tests/fixtures/polls/empty-results.html"),
        ),
    ] {
        router = router
            .route(
                &format!("/poll-visual/reference/{name}"),
                get(move || async move { Html(reference) }),
            )
            .route(
                &format!("/poll-visual/production/{name}"),
                get(move || async move { Html(page(name)) }),
            );
    }
    router
}
