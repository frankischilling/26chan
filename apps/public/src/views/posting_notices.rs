//! Source S_RULES families, rendered through a fixed escaped template.
use board_store::Board;

pub struct Notices {
    pub visible: bool,
    pub code: bool,
    pub sjis: bool,
    pub math: bool,
    pub dice: bool,
    pub pdf: bool,
    pub types: bool,
    pub size: bool,
    pub minimum: bool,
    pub maximum: bool,
    pub advice: bool,
    pub flash_unavailable: bool,
}
pub fn for_board(board: &Board, media: bool) -> Notices {
    let slug = board.slug.as_str();
    let uploads = media && !board.text_only && board.image_limit > 0 && !board.upload_board;
    Notices {
        visible: slug != "test",
        code: slug == "g" && board.comment_code_spacing,
        sjis: slug == "jp" && board.comment_sjis_spacing,
        math: slug == "sci" && board.math_tags,
        dice: matches!(slug, "qst" | "tg") && board.dice_roll,
        pdf: matches!(slug, "gd" | "po" | "qst" | "sci" | "tg") && board.pdf_enabled && uploads,
        types: matches!(slug, "gif" | "wsg") && uploads,
        size: matches!(slug, "hr" | "trv" | "w" | "wg") && uploads,
        minimum: matches!(slug, "hc" | "hr" | "s" | "w" | "wg") && uploads,
        maximum: matches!(slug, "hr" | "trv") && uploads,
        advice: slug == "adv",
        flash_unavailable: board.upload_board,
    }
}
