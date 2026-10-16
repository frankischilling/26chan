//! Private accepted-post comparison metadata; it grants no posting authority.
use crate::StoreError;
use board_domain::{
    content_admission::network::{ProviderValue, hints::DerivedHints},
    poster_id::{PosterIdKey, PublicPostingRateIdentity},
};
use sqlx::PgConnection;
use std::net::IpAddr;

pub(crate) struct Capture<'a> {
    pub board: &'a str,
    pub parent: i64,
    pub actor: &'a PublicPostingRateIdentity,
    pub key: &'a PosterIdKey,
    pub peer: IpAddr,
    pub hints: Option<&'a DerivedHints>,
}

pub(crate) async fn install_metadata(
    connection: &mut PgConnection,
    capture: Capture<'_>,
) -> Result<(), StoreError> {
    let metadata = match (
        capture.key.automatic_range_prefix(capture.peer),
        capture.hints.map(|hints| hints.signatures().browser_id),
    ) {
        (Some(prefix), Some(ProviderValue::Available(Some(browser))))
            if browser.starts_with('1') =>
        {
            let hex = |value: &[u8]| -> String {
                value.iter().map(|byte| format!("{byte:02x}")).collect()
            };
            serde_json::json!({
                "board": capture.board,
                "parent": capture.parent.to_string(),
                "actor": hex(capture.actor.as_bytes()),
                "browser": browser,
                "prefix": hex(prefix.as_bytes()),
            })
            .to_string()
        }
        _ => String::new(),
    };
    // Always replace pooled-session state, including when no evidence exists.
    // The insert trigger binds the context to its new row and consumes it once.
    sqlx::query("SELECT set_config('board.automatic_range_metadata',$1,true)")
        .bind(metadata)
        .execute(connection)
        .await?;
    Ok(())
}
