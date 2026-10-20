//! Quotes and view capabilities do not carry payment acknowledgement authority.
use crate::{AppState, access::error};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_domain::{challenge_secrets::CheckoutToken, checkout::Money};
use board_store::verification::checkout::RenewalOrder;
use board_verification_client::protocol::{
    self, CheckoutCreated, CheckoutOffer, CheckoutState, CheckoutStatus,
};
fn unavailable() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE)
}
fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}
pub(crate) async fn offer(State(state): State<AppState>) -> Response {
    match state.store.checkout_offer().await {
        Ok(offer)
            if offer.enabled
                && offer.provider == "sandbox"
                && Money::new(offer.amount_minor, &offer.currency).is_some() =>
        {
            Json(CheckoutOffer::Ready {
                provider: offer.provider,
                amount_minor: offer.amount_minor,
                currency: offer.currency,
            })
            .into_response()
        }
        Ok(_) => Json(CheckoutOffer::Unavailable {}).into_response(),
        Err(_) => unavailable(),
    }
}
pub(crate) async fn renewal(
    State(state): State<AppState>,
    Json(input): Json<protocol::CheckoutRenewal>,
) -> Response {
    let Some(reference) = state.key.pass_renewal(&input.reference) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(network) = state.key.pass_networks(input.peer) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(view) = state.key.checkout_view(&input.nonce, &reference) else {
        return error(StatusCode::BAD_REQUEST);
    };
    if Money::new(input.amount_minor, &input.currency).is_none() {
        return error(StatusCode::BAD_REQUEST);
    }
    let offer = match state.store.checkout_offer().await {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    if offer.provider != "sandbox" {
        return Json(CheckoutCreated::Unavailable {}).into_response();
    }
    let hash = view.storage_hash();
    let hex: String = hash[..16].iter().map(|b| format!("{b:02x}")).collect();
    let order = format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    );
    let object = format!("sandbox_{order}");
    let result = state
        .store
        .create_renewal_order(&RenewalOrder {
            id: &order,
            view: &hash,
            peer: &network.peer,
            reference: &reference,
            amount: input.amount_minor,
            currency: &input.currency,
            object: &object,
        })
        .await;
    match result.as_deref() {
        Ok("created") => Json(CheckoutCreated::Ready {
            order,
            view: view.encode(),
        })
        .into_response(),
        Ok("unavailable" | "refused") => Json(CheckoutCreated::Unavailable {}).into_response(),
        Ok("price_changed") => Json(CheckoutCreated::PriceChanged {}).into_response(),
        Ok("flood") => Json(CheckoutCreated::Flood {}).into_response(),
        Ok("capacity") => Json(CheckoutCreated::Capacity {}).into_response(),
        _ => unavailable(),
    }
}
pub(crate) async fn status(
    State(state): State<AppState>,
    Json(input): Json<protocol::CheckoutView>,
) -> Response {
    let Some(view) = CheckoutToken::parse(&input.view) else {
        return Json(CheckoutState::Missing {}).into_response();
    };
    if !uuid(&input.order) {
        return Json(CheckoutState::Missing {}).into_response();
    }
    let row = match state
        .store
        .checkout_state(&input.order, &view.storage_hash())
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return Json(CheckoutState::Missing {}).into_response(),
        Err(_) => return unavailable(),
    };
    let status = match row.state.as_str() {
        "pending" => CheckoutStatus::Pending,
        "paid" => CheckoutStatus::Paid,
        "declined" => CheckoutStatus::Declined,
        "cancelled" => CheckoutStatus::Cancelled,
        "refunded" => CheckoutStatus::Refunded,
        "disputed" => CheckoutStatus::Disputed,
        "expired" => CheckoutStatus::Expired,
        _ => return unavailable(),
    };
    let identifier = if row.product == "pass_purchase" && row.fulfilled {
        let contact = match state
            .store
            .purchase_contact(&input.order, &view.storage_hash())
            .await
        {
            Ok(Some(bytes)) => bytes,
            _ => return unavailable(),
        };
        // The narrow view-bound function prefixes the encrypted contact with
        // its identity AAD. Only the public identifier leaves this service.
        let Some((identity, contact)) = decode_purchase_contact(&state, contact) else {
            return unavailable();
        };
        if state.key.pass_identifier(contact.identifier()) != Some(identity) {
            return unavailable();
        }
        Some(contact.identifier().to_owned())
    } else {
        None
    };
    Json(CheckoutState::Ready {
        status,
        product: row.product,
        identifier,
        provider: row.provider,
        amount_minor: row.amount_minor,
        currency: row.currency,
        fulfilled: row.fulfilled,
        review_required: row.review_required,
    })
    .into_response()
}

fn decode_purchase_contact(
    state: &AppState,
    bytes: Vec<u8>,
) -> Option<([u8; 32], board_domain::pass_mail::Contact)> {
    let identity: [u8; 32] = bytes.get(..32)?.try_into().ok()?;
    let board_domain::pass_mail::Payload::Contact(contact) =
        state.mail_key.open_pass(&identity, bytes.get(32..)?)?
    else {
        return None;
    };
    Some((identity, contact))
}
