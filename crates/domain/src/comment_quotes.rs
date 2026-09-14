use std::borrow::Cow;

// Source imgboard.php:5378. This is a stored-text rewrite, not quote parsing:
// leave digit bytes intact, including zero and values outside an integer range.
pub(super) fn same_board_quotes<'a>(text: &'a str, board: &str) -> Cow<'a, str> {
    let Ok(board) = crate::BoardSlug::parse(board) else {
        return Cow::Borrowed(text);
    };
    let prefix = format!(">>>/{}/", board.as_str());
    let mut result = None::<String>;
    let mut cursor = 0;
    let mut copied = 0;
    while let Some(offset) = text[cursor..].find(&prefix) {
        let start = cursor + offset;
        let end = start + prefix.len();
        if text.as_bytes().get(end).is_some_and(u8::is_ascii_digit) {
            let result = result.get_or_insert_with(|| String::with_capacity(text.len()));
            result.push_str(&text[copied..start]);
            result.push_str(">>");
            copied = end;
        }
        cursor = end;
    }
    match result {
        Some(mut result) => {
            result.push_str(&text[copied..]);
            Cow::Owned(result)
        }
        None => Cow::Borrowed(text),
    }
}
