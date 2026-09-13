use axum::{extract::Query, http::Uri};
use board_store::BoardSnapshot;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
pub enum Order {
    #[default]
    #[serde(rename = "alt")]
    Bump,
    #[serde(rename = "absdate")]
    LastReply,
    #[serde(rename = "date")]
    Creation,
    #[serde(rename = "r")]
    Replies,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    #[default]
    Small,
    Large,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Teaser {
    Off,
    #[default]
    On,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub order: Order,
    pub size: Size,
    pub teaser: Teaser,
    pub q: String,
}

impl Options {
    pub fn parse(uri: &Uri) -> Option<Self> {
        if uri.query().is_some_and(|query| query.len() > 2048) {
            return None;
        }
        let Query(options) = Query::<Self>::try_from_uri(uri).ok()?;
        if options.q.chars().count() > 128 || options.q.chars().any(char::is_control) {
            return None;
        }
        Some(options)
    }

    pub fn order_value(&self) -> &'static str {
        match self.order {
            Order::Bump => "alt",
            Order::LastReply => "absdate",
            Order::Creation => "date",
            Order::Replies => "r",
        }
    }

    pub fn large(&self) -> bool {
        self.size == Size::Large
    }

    pub fn show_teaser(&self) -> bool {
        self.teaser == Teaser::On
    }

    pub fn class(&self) -> &'static str {
        match (self.size, self.teaser) {
            (Size::Small, Teaser::Off) => "small",
            (Size::Small, Teaser::On) => "extended-small",
            (Size::Large, Teaser::Off) => "large",
            (Size::Large, Teaser::On) => "extended-large",
        }
    }

    pub fn apply(&self, snapshot: &mut BoardSnapshot) {
        if !self.q.is_empty() {
            let query = self.q.to_lowercase();
            snapshot.threads.retain(|preview| {
                preview
                    .posts
                    .iter()
                    .find(|post| post.id == preview.thread.id)
                    .is_some_and(|post| {
                        post.subject.to_lowercase().contains(&query)
                            || post.comment.to_lowercase().contains(&query)
                            || post.attachment.as_ref().is_some_and(|file| {
                                !file.file_deleted && file.filename.to_lowercase().contains(&query)
                            })
                    })
            });
        }
        snapshot.threads.sort_by(|a, b| {
            b.thread
                .sticky
                .cmp(&a.thread.sticky)
                .then_with(|| match self.order {
                    Order::Bump => b
                        .thread
                        .bumped_at
                        .cmp(&a.thread.bumped_at)
                        .then_with(|| b.thread.id.cmp(&a.thread.id)),
                    Order::Creation => b.thread.id.cmp(&a.thread.id),
                    Order::LastReply => b
                        .latest_reply_id
                        .cmp(&a.latest_reply_id)
                        .then_with(|| a.thread.id.cmp(&b.thread.id)),
                    Order::Replies => b
                        .visible_posts
                        .cmp(&a.visible_posts)
                        .then_with(|| a.thread.id.cmp(&b.thread.id)),
                })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn only_bounded_finite_options_are_accepted() {
        for query in [
            "order=invalid",
            "size=wide",
            "teaser=yes",
            "order=r&order=alt",
            "unknown=x",
            "q=%00",
            "q=%0A",
        ] {
            assert!(
                Options::parse(&format!("/demo/catalog?{query}").parse().unwrap()).is_none(),
                "{query}"
            );
        }
        assert!(
            Options::parse(
                &format!("/demo/catalog?q={}", "a".repeat(129))
                    .parse()
                    .unwrap()
            )
            .is_none()
        );
        assert!(
            Options::parse(
                &format!("/demo/catalog?q={}", "%61".repeat(700))
                    .parse()
                    .unwrap()
            )
            .is_none()
        );
        let options = Options::parse(
            &"/demo/catalog?order=absdate&size=large&teaser=off&q=%3Cscript%3E"
                .parse()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(options.order, Order::LastReply);
        assert_eq!(options.class(), "large");
        assert_eq!(options.q, "<script>");
        assert_eq!(Options::default().class(), "extended-small");
    }

    proptest! {
        #[test]
        fn decoded_queries_remain_bounded(query in ".{0,300}") {
            let encoded: String = url::form_urlencoded::Serializer::new(String::new()).append_pair("q", &query).finish();
            let uri = format!("/demo/catalog?{encoded}").parse().unwrap();
            if let Some(options) = Options::parse(&uri) {
                prop_assert!(options.q.chars().count() <= 128);
                prop_assert!(!options.q.chars().any(char::is_control));
                prop_assert_eq!(options.q, query);
            }
        }
    }
}
