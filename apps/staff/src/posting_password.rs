use crate::AppError;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use std::sync::{Arc, LazyLock};

static HASHES: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(2)));

// Match the public writer's fixed work profile. Stored credentials cannot
// choose the memory, iterations or output size used for OP verification.
fn verify(password: &str, encoded: &str) -> bool {
    encoded.len() <= 256
        && PasswordHash::new(encoded).is_ok_and(|hash| {
            hash.algorithm.as_str() == "argon2id"
                && hash.version == Some(19)
                && hash.params.iter().count() == 3
                && hash.params.get_decimal("m") == Some(19_456)
                && hash.params.get_decimal("t") == Some(2)
                && hash.params.get_decimal("p") == Some(1)
                && hash.hash.as_ref().is_some_and(|output| output.len() == 32)
                && Argon2::default()
                    .verify_password(password.as_bytes(), &hash)
                    .is_ok()
        })
}

pub(crate) async fn prepare(
    password: String,
    op_hash: Option<String>,
) -> Result<(String, Option<[u8; 32]>), AppError> {
    if !(8..=128).contains(&password.len()) {
        return Err(AppError::Posting(
            "Deletion password must contain 8 to 128 bytes.".into(),
        ));
    }
    let permit = HASHES
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::Capacity)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let proof = op_hash
            .as_deref()
            .filter(|hash| verify(&password, hash))
            .map(|hash| <[u8; 32]>::from(Sha256::digest(hash.as_bytes())));
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|hash| (hash.to_string(), proof))
            .map_err(|_| AppError::Internal)
    })
    .await
    .map_err(|_| AppError::Internal)?
}
