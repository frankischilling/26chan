use board_domain::{poll_voting::COOKIE_SECONDS, poster_id::PosterIdKey};

const NOW: i64 = 1_700_000_000;

#[test]
fn poll_voter_public_api_round_trips_across_independent_requests() {
    let key = PosterIdKey::parse(&"31".repeat(32))
        .unwrap()
        .poll_voting_key();
    let voter = key.generate_voter(NOW).unwrap();
    assert!(voter.credential().is_ascii());
    assert!(!voter.credential().contains(';'));
    let parsed = key.parse_voter(voter.credential(), NOW + 1).unwrap();
    assert_eq!(parsed.credential(), voter.credential());

    let form = key.form_token(&parsed, 42, NOW).unwrap();
    assert!(key.verify_form_token(&parsed, 42, &form, NOW + 1));
    assert!(!key.verify_form_token(&parsed, 43, &form, NOW + 1));
    assert!(!key.verify_form_token(&parsed, 42, &form, NOW + 1_800));
    assert!(key.voter_hash(&parsed, 42).unwrap() == key.voter_hash(&voter, 42).unwrap());
    assert!(key.voter_hash(&parsed, 42).unwrap() != key.voter_hash(&parsed, 43).unwrap());
    assert_eq!(COOKIE_SECONDS, 31_536_000);
    assert!(
        key.parse_voter(voter.credential(), NOW + COOKIE_SECONDS as i64)
            .is_none()
    );
}

#[test]
fn poll_voter_public_api_rejects_client_forgery_and_key_rotation() {
    let old = PosterIdKey::parse(&"31".repeat(32))
        .unwrap()
        .poll_voting_key();
    let rotated = PosterIdKey::parse(&"41".repeat(32))
        .unwrap()
        .poll_voting_key();
    let voter = old.generate_voter(NOW).unwrap();
    let form = old.form_token(&voter, i64::MAX, NOW).unwrap();
    assert!(old.verify_form_token(&voter, i64::MAX, &form, NOW));
    assert!(rotated.parse_voter(voter.credential(), NOW).is_none());
    assert!(!rotated.verify_form_token(&voter, i64::MAX, &form, NOW));
    assert!(rotated.voter_hash(&voter, i64::MAX).is_err());
    assert!(old.parse_voter("client-chosen", NOW).is_none());
    assert!(old.form_token(&voter, 0, NOW).is_err());
    assert!(old.voter_hash(&voter, -1).is_err());
}
