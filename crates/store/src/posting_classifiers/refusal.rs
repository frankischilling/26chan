use std::fmt;

#[derive(Debug)]
pub struct Refusal {
    pub(crate) quiet: Option<i64>,
    pub(crate) image: bool,
    pub(crate) cookie_refreshed: bool,
}
impl Refusal {
    pub fn quiet_post(&self) -> Option<i64> {
        self.quiet
    }
    pub fn cookie_refreshed(&self) -> bool {
        self.cookie_refreshed
    }
    pub fn message(&self) -> &'static str {
        if self.image {
            "Uploading files from your IP range has been temporarily blocked due to abuse. Please verify your e-mail address or try again later."
        } else {
            crate::AUTOMATIC_RANGE_REJECTION
        }
    }
}
impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(if self.quiet.is_some() {
            "Post accepted."
        } else {
            self.message()
        })
    }
}
