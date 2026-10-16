//! Private untouched-input identity, separate from every processed source profile.
use crate::{
    StoreError,
    media::{MediaQueue, validate_hex},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputComponent {
    Upload,
    PairedImage,
}

impl InputComponent {
    fn as_str(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::PairedImage => "paired-image",
        }
    }
}

/// Measured by the trusted host from the immutable bytes sent for processing.
/// For paired input this describes only the image, never the transport frame.
#[derive(Clone, PartialEq, Eq)]
pub struct InputReceipt {
    pub component: InputComponent,
    pub sha256: String,
    pub bytes: i64,
    pub md5: [u8; 16],
}

impl InputReceipt {
    pub(super) async fn require_on(
        &self,
        connection: &mut sqlx::PgConnection,
        job: &str,
        token: &str,
        asset: &str,
    ) -> Result<(), StoreError> {
        self.validate()?;
        sqlx::query("SELECT media.require_input_receipt($1,$2,$3,$4,$5,$6,$7)")
            .bind(job)
            .bind(token)
            .bind(asset)
            .bind(&self.sha256)
            .bind(self.bytes)
            .bind(self.md5.as_slice())
            .bind(self.component.as_str())
            .execute(connection)
            .await?;
        Ok(())
    }

    pub(super) fn validate(&self) -> Result<(), StoreError> {
        validate_hex(&self.sha256, 64)?;
        if !(1..=10_485_760).contains(&self.bytes) {
            return Err(StoreError::Invalid("Invalid original input bounds."));
        }
        Ok(())
    }
}

impl MediaQueue {
    /// Call under the publication storage lock after reservation and before
    /// installing files or approving. An uncertain result leaves the reservation
    /// recoverable; never remove a possibly approved asset to retry this step.
    pub async fn record_input_receipt(
        &self,
        job: &str,
        token: &str,
        asset: &str,
        receipt: &InputReceipt,
    ) -> Result<(), StoreError> {
        validate_hex(job, 32)?;
        validate_hex(token, 32)?;
        validate_hex(asset, 32)?;
        receipt.validate()?;
        sqlx::query("SELECT media.record_input_receipt($1,$2,$3,$4,$5,$6,$7)")
            .bind(job)
            .bind(token)
            .bind(asset)
            .bind(&receipt.sha256)
            .bind(receipt.bytes)
            .bind(receipt.md5.as_slice())
            .bind(receipt.component.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_requires_canonical_integrity_hash_and_bounded_length() {
        for bytes in [1, 8_388_608, 10_485_760] {
            assert!(
                InputReceipt {
                    component: InputComponent::Upload,
                    sha256: "a".repeat(64),
                    bytes,
                    md5: [0; 16]
                }
                .validate()
                .is_ok()
            );
        }
        for bytes in [i64::MIN, -1, 0, 10_485_761, i64::MAX] {
            assert!(
                InputReceipt {
                    component: InputComponent::Upload,
                    sha256: "a".repeat(64),
                    bytes,
                    md5: [0; 16]
                }
                .validate()
                .is_err()
            );
        }
        for sha256 in [
            "A".repeat(64),
            "a".repeat(63),
            "a".repeat(65),
            format!("{}\n", "a".repeat(63)),
        ] {
            assert!(
                InputReceipt {
                    component: InputComponent::Upload,
                    sha256,
                    bytes: 1,
                    md5: [0; 16]
                }
                .validate()
                .is_err()
            );
        }
    }
}
