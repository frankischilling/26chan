//! Source server choices and catalog suffixes have independent policies.
use board_store::Board;
use rand_core::{OsRng, RngCore};

pub(crate) const ASSET_NAMES: &[&str] = &[
    "spoiler-a1.png",
    "spoiler-co1.png",
    "spoiler-co2.png",
    "spoiler-co3.png",
    "spoiler-co4.png",
    "spoiler-co5.png",
    "spoiler-jp1.png",
    "spoiler-lit1.png",
    "spoiler-m1.png",
    "spoiler-m2.png",
    "spoiler-m3.png",
    "spoiler-m4.png",
    "spoiler-mlp1.png",
    "spoiler-news1.png",
    "spoiler-s4s1.png",
    "spoiler-s4s2.png",
    "spoiler-s4s3.png",
    "spoiler-s4s4.png",
    "spoiler-s4s5.png",
    "spoiler-s4s6.png",
    "spoiler-tg1.png",
    "spoiler-tg2.png",
    "spoiler-tv1.png",
    "spoiler-tv2.png",
    "spoiler-tv3.png",
    "spoiler-tv4.png",
    "spoiler-tv5.png",
    "spoiler-v1.png",
    "spoiler-vg1.png",
    "spoiler-vm1.png",
    "spoiler-vmg1.png",
    "spoiler-vmg2.png",
    "spoiler-vmg3.png",
    "spoiler-vp1.png",
    "spoiler-vr1.png",
    "spoiler-vr2.png",
    "spoiler-vrpg1.png",
    "spoiler-vrpg2.png",
    "spoiler-vrpg3.png",
    "spoiler-vst.png",
    "spoiler-vst1.png",
    "spoiler-vt1.png",
    "spoiler-vt2.png",
    "spoiler-vt3.png",
    "spoiler.png",
];

pub fn thumbnail_at(board: &Board, index: usize) -> String {
    let name = board
        .spoiler_thumbnail_assets
        .get(index)
        .map(String::as_str)
        .unwrap_or("spoiler.png");
    let name = if ASSET_NAMES.contains(&name) {
        name
    } else {
        "spoiler.png"
    };
    format!("/static/catalog/{name}")
}

pub fn choose_thumbnail(board: &Board) -> String {
    let length = board.spoiler_thumbnail_assets.len().clamp(1, 64) as u64;
    let ceiling = u64::MAX - u64::MAX % length;
    let index = loop {
        let value = OsRng.next_u64();
        if value < ceiling {
            break (value % length) as usize;
        }
    };
    thumbnail_at(board, index)
}

pub fn catalog_thumbnail(board: &Board) -> String {
    if board.comment_spoiler_cleanup && board.custom_spoiler_count > 0 {
        format!(
            "/static/catalog/spoiler-{}{}.png",
            board.slug, board.custom_spoiler_count
        )
    } else {
        "/static/catalog/spoiler.png".to_owned()
    }
}
