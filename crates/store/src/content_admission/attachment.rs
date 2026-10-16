//! Capability-scoped metadata held under the posting transaction's job lock.
use crate::{
    StoreError,
    post_media::{self, NewAttachment},
};
use sqlx::PgConnection;

type Metadata = (String, Option<String>, Option<Vec<u8>>, Option<bool>);

pub(crate) struct Target<'a> {
    pub board: &'a str,
    pub parent: i64,
    pub authorized: bool,
    pub badged: bool,
}

pub(crate) struct Preflight<'a> {
    attachment: &'a NewAttachment,
    target: Target<'a>,
    metadata: Metadata,
}

impl<'a> Preflight<'a> {
    pub async fn begin(
        connection: &mut PgConnection,
        attachment: &'a NewAttachment,
        target: Target<'a>,
    ) -> Result<Self, StoreError> {
        let metadata = read(connection, attachment, &target).await?;
        Ok(Self {
            attachment,
            target,
            metadata,
        })
    }

    pub fn filename(&self) -> &str {
        &self.metadata.0
    }

    /// A filter evaluation may wait for bounded native work. Check expiry again
    /// before committing its effects, without consuming the one-use attachment.
    pub async fn revalidate(&self, connection: &mut PgConnection) -> Result<(), StoreError> {
        if read(connection, self.attachment, &self.target).await? != self.metadata {
            return Err(StoreError::Conflict(
                "Attachment admission metadata changed.",
            ));
        }
        Ok(())
    }
}

async fn read(
    connection: &mut PgConnection,
    attachment: &NewAttachment,
    target: &Target<'_>,
) -> Result<Metadata, StoreError> {
    sqlx::query_as("SELECT filename,normalized_md5,original_md5,original_eligible FROM content.attachment_upload_filter_metadata($1,$2,$3,$4,$5,$6)")
        .bind(&attachment.upload.id).bind(&attachment.upload.capability)
        .bind(target.board).bind(target.parent).bind(target.authorized).bind(target.badged)
        .fetch_one(connection).await.map_err(post_media::scoped_error)
}
