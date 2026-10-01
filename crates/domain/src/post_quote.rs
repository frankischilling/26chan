//! A post reference retains its lexical label independently of its numeric
//! destination. Only validated board identifiers can become route segments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostQuote {
    pub(crate) board: Option<String>,
    pub(crate) id: u64,
    pub(crate) label: String,
}

impl PostQuote {
    pub fn href(&self, current_board: &str) -> String {
        format!(
            "/{}/post/{}",
            self.board.as_deref().unwrap_or(current_board),
            self.id
        )
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn board(&self) -> Option<&str> {
        self.board.as_deref()
    }

    pub fn board_label(&self) -> &str {
        self.board.as_deref().unwrap_or("")
    }

    pub fn digits(&self) -> &str {
        if self.board.is_some() {
            self.label
                .rsplit('/')
                .next()
                .expect("validated post reference")
        } else {
            &self.label[2..]
        }
    }
}
