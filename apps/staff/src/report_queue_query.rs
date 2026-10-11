//! Strict, bounded query grammar for the retained report inventory.
use crate::{AppError, report_queue::ReportFilter};

pub fn parse(raw: Option<&str>) -> Result<ReportFilter, AppError> {
    let raw = raw.unwrap_or_default();
    if raw.len() > 256 {
        return Err(AppError::Invalid);
    }
    if raw.is_empty() {
        return Ok(ReportFilter::All);
    }
    let mut board = None;
    for field in raw.split('&') {
        let (key, value) = field.split_once('=').ok_or(AppError::Invalid)?;
        let key = decode(key)?;
        let value = decode(value)?;
        if key != "board" || board.is_some() {
            return Err(AppError::Invalid);
        }
        board = Some(value);
    }
    match board.as_deref().ok_or(AppError::Invalid)? {
        "" => Ok(ReportFilter::All),
        "_ws_" => Ok(ReportFilter::Worksafe),
        "_nws_" => Ok(ReportFilter::NotWorksafe),
        value => {
            let board = board_domain::BoardSlug::parse(value).map_err(|_| AppError::Invalid)?;
            Ok(ReportFilter::Board(board.as_str().to_owned()))
        }
    }
}

fn decode(value: &str) -> Result<String, AppError> {
    let mut result = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        match byte {
            b'%' => {
                let high = bytes.next().and_then(hex).ok_or(AppError::Invalid)?;
                let low = bytes.next().and_then(hex).ok_or(AppError::Invalid)?;
                result.push(high * 16 + low);
            }
            b'+' => result.push(b' '),
            byte => result.push(byte),
        }
    }
    String::from_utf8(result).map_err(|_| AppError::Invalid)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_supported_filters() {
        for raw in [None, Some(""), Some("board=")] {
            assert_eq!(parse(raw).unwrap(), ReportFilter::All);
        }
        assert_eq!(parse(Some("board=_ws_")).unwrap(), ReportFilter::Worksafe);
        assert_eq!(
            parse(Some("board=_nws_")).unwrap(),
            ReportFilter::NotWorksafe
        );
        assert_eq!(
            parse(Some("%62oard=%61b1")).unwrap(),
            ReportFilter::Board("ab1".into())
        );
    }

    #[test]
    fn rejects_ambiguous_unknown_and_malformed_queries() {
        for raw in [
            "board",
            "ignored",
            "ignored=1",
            "cleared_only=1",
            "unlock=1",
            "limit=2",
            "board=a&board=b",
            "board=&%62oard=a",
            "%62oard=a&bo%61rd=b",
            "board=a&",
            "&board=a",
            "board=a&&board=b",
            "Board=a",
            "board=a;ignored=1",
            "board=%",
            "board=%0",
            "board=%gg",
            "bo%ard=a",
            "board=%ff",
            "board=%c0%af",
            "board=%00",
            "board=a+b",
            "board=%2f",
            "board=UPPER",
            "board=polls",
            "board=abcdefghijk",
            "board=_other_",
            "board=a=b",
        ] {
            assert!(parse(Some(raw)).is_err(), "accepted {raw}");
        }
        assert!(parse(Some(&format!("board={}", "%61".repeat(84)))).is_err());
    }
}
