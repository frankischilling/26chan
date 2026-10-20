//! Native sandbox sales; account authority remains in the payment transaction.
use crate::{AppState, access::error};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_domain::{
    challenge_secrets::{PassRecoveryToken, ReceiptToken},
    checkout::{Money, new_pass_identifier},
    pass_mail::{Contact, Payload, Recipient},
};
use board_store::verification::checkout::{PurchaseOrder, PurchasePreparation};
use board_verification_client::protocol::{CheckoutCreated, CheckoutOffer, CheckoutPurchase};
use ring::rand::{SecureRandom, SystemRandom};
fn unavailable() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE)
}
pub(crate) async fn offer(State(state): State<AppState>) -> Response {
    match state.store.purchase_offer().await {
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
pub(crate) async fn create(
    State(state): State<AppState>,
    Json(input): Json<CheckoutPurchase>,
) -> Response {
    if Recipient::parse(&input.email).is_none()
        || Money::new(input.amount_minor, &input.currency).is_none()
    {
        return error(StatusCode::BAD_REQUEST);
    }
    let Some(recipient) = Recipient::owner(&input.email, input.gift_email.as_deref()) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(recipient_hash) = state.key.pass_recipient(recipient.address()) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(network) = state.key.pass_networks(input.peer) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(view) = state
        .key
        .checkout_purchase_view(&input.nonce, &recipient_hash)
    else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Ok(ticket) = ReceiptToken::generate() else {
        return unavailable();
    };
    match state.store.purchase_offer().await {
        Ok(offer) if offer.provider == "sandbox" => {}
        Ok(_) => return Json(CheckoutCreated::Unavailable {}).into_response(),
        Err(_) => return unavailable(),
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
        .begin_purchase(&PurchaseOrder {
            id: &order,
            view: &hash,
            peer: &network.peer,
            recipient: &recipient_hash,
            amount: input.amount_minor,
            currency: &input.currency,
            object: &object,
            delayed: input.delayed,
            ticket: &ticket.storage_hash(),
        })
        .await;
    match result.as_deref() {
        Ok("created") => {
            return Json(CheckoutCreated::Ready {
                order,
                view: view.encode(),
            })
            .into_response();
        }
        Ok("prepare") => {}
        Ok("unavailable" | "refused") => {
            return Json(CheckoutCreated::Unavailable {}).into_response();
        }
        Ok("price_changed") => return Json(CheckoutCreated::PriceChanged {}).into_response(),
        Ok("flood") => return Json(CheckoutCreated::Flood {}).into_response(),
        Ok("capacity") => return Json(CheckoutCreated::Capacity {}).into_response(),
        _ => return unavailable(),
    }
    let Ok(permit) = state.pass_hashing.clone().try_acquire_owned() else {
        return Json(CheckoutCreated::Capacity {}).into_response();
    };
    let (Ok(identifier), Ok(reference), Ok(setup), Ok(initial)) = (
        new_pass_identifier(),
        ReceiptToken::generate(),
        PassRecoveryToken::generate(),
        ReceiptToken::generate(),
    ) else {
        return unavailable();
    };
    let encoded = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut salt = [0; 16];
        SystemRandom::new().fill(&mut salt).ok()?;
        let salt = SaltString::encode_b64(&salt).ok()?;
        // This random initial secret is never returned or mailed. Only the
        // one-use setup link lets the recipient select a usable PIN.
        Argon2::default()
            .hash_password(initial.encode().as_bytes(), &salt)
            .ok()
            .map(|hash| hash.to_string())
    })
    .await
    .ok()
    .flatten();
    let Some(encoded) = encoded else {
        return unavailable();
    };
    let reference = reference.encode();
    let Some(identity) = state.key.pass_identifier(&identifier) else {
        return unavailable();
    };
    let Some(reference_hash) = state.key.pass_renewal(&reference) else {
        return unavailable();
    };
    let Some(contact) = Contact::new(recipient, &identifier, &reference) else {
        return unavailable();
    };
    let Some(encrypted) = state
        .mail_key
        .seal_pass(&identity, &Payload::Contact(contact))
    else {
        return unavailable();
    };
    let Some(Payload::Contact(contact)) = state.mail_key.open_pass(&identity, &encrypted) else {
        return unavailable();
    };
    let setup_hash = setup.storage_hash();
    let Some(mail) = state.mail_key.seal_pass(
        &setup_hash,
        &Payload::Setup {
            contact,
            token: setup,
        },
    ) else {
        return unavailable();
    };
    match state
        .store
        .prepare_purchase(&PurchasePreparation {
            id: &order,
            ticket: &ticket.storage_hash(),
            identity: &identity,
            pin: &encoded,
            contact: &encrypted,
            reference: &reference_hash,
            setup: &setup_hash,
            mail: &mail,
        })
        .await
        .as_deref()
    {
        Ok("ready") => Json(CheckoutCreated::Ready {
            order,
            view: view.encode(),
        })
        .into_response(),
        _ => unavailable(),
    }
}
