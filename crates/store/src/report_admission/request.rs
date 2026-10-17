//! Qualified native report-score evidence. Missing or mismatched observations
//! remain unknown; this type grants neither staff nor Pass authority.
use board_domain::content_admission::{network::ProviderValue, threat::NativeReportScore};
use chrono::{DateTime, Utc};
use std::net::IpAddr;

pub struct RequestFacts {
    at: DateTime<Utc>,
    peer: Option<IpAddr>,
    board: String,
    post: i64,
    score: ProviderValue<NativeReportScore>,
}
impl RequestFacts {
    pub fn new(
        at: DateTime<Utc>,
        peer: Option<IpAddr>,
        board: &str,
        post: i64,
        score: ProviderValue<NativeReportScore>,
    ) -> Self {
        Self {
            at,
            peer: peer.map(|peer| peer.to_canonical()),
            board: board.into(),
            post,
            score,
        }
    }
    pub(crate) fn threshold(
        &self,
        at: DateTime<Utc>,
        peer: Option<IpAddr>,
        board: &str,
        post: i64,
    ) -> Option<bool> {
        if self.at != at
            || self.peer != peer.map(|peer| peer.to_canonical())
            || self.board != board
            || self.post != post
        {
            return None;
        }
        match self.score {
            ProviderValue::Available(score) => Some(score.at_least_point_four()),
            ProviderValue::Unavailable => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ProviderValue::{Available as A, Unavailable as U};
    use board_domain::content_admission::threat::{self, Request};

    #[test]
    fn observations_bind_exact_request_scope_and_canonical_peer() {
        let now = Utc::now();
        let peer = Some("192.0.2.1".parse().unwrap());
        let score = threat::score_native_report(
            A(Request {
                server: &[],
                cookies: &[],
                challenge: None,
                response: None,
                request_time: now.timestamp(),
                country: None,
                is_op: true,
                multipart: false,
            }),
            A(false),
        );
        let A(value) = score else {
            panic!("complete reporting fixture");
        };
        let facts = RequestFacts::new(now, peer, "g", 10, score);
        assert_eq!(
            facts.threshold(now, peer, "g", 10),
            Some(value.at_least_point_four())
        );
        assert_eq!(
            facts.threshold(now, Some("::ffff:192.0.2.1".parse().unwrap()), "g", 10),
            Some(value.at_least_point_four())
        );
        assert_eq!(
            facts.threshold(now + chrono::Duration::nanoseconds(1), peer, "g", 10),
            None
        );
        assert_eq!(facts.threshold(now, None, "g", 10), None);
        assert_eq!(
            facts.threshold(now, Some("192.0.2.2".parse().unwrap()), "g", 10),
            None
        );
        assert_eq!(facts.threshold(now, peer, "a", 10), None);
        assert_eq!(facts.threshold(now, peer, "g", 11), None);
        assert_eq!(
            RequestFacts::new(now, peer, "g", 10, U).threshold(now, peer, "g", 10),
            None
        );
    }
}
