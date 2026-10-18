//! Server-owned transport evidence bound to one posting request. No raw header
//! set, credential or full cookie collection is retained here.
use crate::PostingContext;
use board_domain::content_admission::{network::ProviderValue, threat::Score};
use chrono::{DateTime, Utc};
use std::net::IpAddr;

pub struct RequestFacts {
    request_start: DateTime<Utc>,
    peer: Option<IpAddr>,
    parent: i64,
    board: String,
    pub(super) bag_match:
        ProviderValue<board_domain::content_admission::classifiers::posting::browser::Match>,
    pub(super) score: ProviderValue<Score>,
    pub(super) country: ProviderValue<[u8; 2]>,
    pub(super) timing_cookie: ProviderValue<Option<Vec<u8>>>,
}
impl RequestFacts {
    pub fn new(
        context: PostingContext,
        parent: i64,
        board: &str,
        score: ProviderValue<Score>,
        country: ProviderValue<[u8; 2]>,
        timing_cookie: ProviderValue<Option<Vec<u8>>>,
    ) -> Self {
        Self {
            request_start: context.request_start,
            peer: context.peer.map(|peer| peer.to_canonical()),
            parent,
            board: board.into(),
            bag_match: ProviderValue::Unavailable,
            score,
            country,
            timing_cookie,
        }
    }
    pub fn with_browser_match(
        mut self,
        matched: ProviderValue<
            board_domain::content_admission::classifiers::posting::browser::Match,
        >,
    ) -> Self {
        self.bag_match = matched;
        self
    }
    pub(super) fn matches(&self, context: PostingContext, board: &str, parent: i64) -> bool {
        self.request_start == context.request_start
            && self.peer == context.peer.map(|peer| peer.to_canonical())
            && self.parent == parent
            && self.board == board
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ProviderValue::{Available as A, Unavailable as U};
    #[test]
    fn facts_stay_bound_to_the_server_request_scope() {
        let context = PostingContext {
            request_start: DateTime::from_timestamp(1000, 123).unwrap(),
            peer: Some("192.0.2.1".parse().unwrap()),
            op_password_proof: None,
        };
        let facts = RequestFacts::new(context, 7, "a", U, A(*b"US"), A(None));
        assert!(facts.matches(context, "a", 7));
        assert!(!facts.matches(context, "b", 7));
        assert!(!facts.matches(context, "a", 8));
        assert!(!facts.matches(
            PostingContext {
                request_start: context.request_start + chrono::Duration::nanoseconds(1),
                ..context
            },
            "a",
            7
        ));
        assert!(!facts.matches(
            PostingContext {
                peer: Some("192.0.2.2".parse().unwrap()),
                ..context
            },
            "a",
            7
        ));
        assert!(facts.matches(
            PostingContext {
                peer: Some("::ffff:192.0.2.1".parse().unwrap()),
                ..context
            },
            "a",
            7
        ));
    }
}
