//! Public renewal forms quote through the verifier; return URLs cannot grant payment.
use crate::{
    AppState,
    handlers::AppError,
    security::RequestPeer,
    verification_form::{CheckoutSecret, Fields, MAX_BODY},
};
use askama::Template;
use axum::{
    Extension,
    body::Bytes,
    extract::{RawQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use board_domain::checkout::Money;
use board_verification_client::{
    VerificationClient,
    protocol::{self, CheckoutCreated, CheckoutOffer, CheckoutState, CheckoutStatus},
};
#[derive(Clone, Copy)]
pub(crate) struct CheckoutPage;
#[derive(Template)]
#[template(path = "pass_checkout.html")]
struct Page<'a> {
    csrf: &'a str,
    reference: &'a str,
    mode: &'a str,
    message: &'a str,
    amount_minor: i64,
    currency: &'a str,
    price: &'a str,
}
fn invalid() -> AppError {
    AppError(StatusCode::BAD_REQUEST, "Invalid Pass checkout form.")
}
fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Pass checkout is unavailable. Try again later.",
    )
}
fn client(state: &AppState) -> Result<&VerificationClient, AppError> {
    state.verification.as_ref().ok_or_else(unavailable)
}
fn reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn render(
    state: &AppState,
    secret: &CheckoutSecret,
    page: &Page<'_>,
) -> Result<Response, AppError> {
    let mut response = crate::output::html(state, page)?;
    secret.append(response.headers_mut(), state.production);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("strict-origin"));
    response.extensions_mut().insert(CheckoutPage);
    Ok(response)
}
fn message(state: &AppState, secret: &CheckoutSecret, text: &str) -> Result<Response, AppError> {
    render(
        state,
        secret,
        &Page {
            csrf: "",
            reference: "",
            mode: "message",
            message: text,
            amount_minor: 0,
            currency: "",
            price: "",
        },
    )
}
pub(crate) async fn page(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
) -> Result<Response, AppError> {
    let fields = Fields::parse(
        query.as_deref().unwrap_or("").as_bytes(),
        &["reset", "renew", "order", "view"],
        512,
    )
    .map_err(|_| invalid())?;
    if fields.has("reset") {
        if fields.has("renew") || fields.has("order") || fields.has("view") {
            return Err(invalid());
        }
        return crate::pass_recovery::page(State(state), RawQuery(query)).await;
    }
    let secret = CheckoutSecret::generate().map_err(|_| unavailable())?;
    if fields.has("renew") {
        if fields.has("order") || fields.has("view") || !reference(fields.get("renew")) {
            return Err(invalid());
        }
        match client(&state)?
            .checkout_offer()
            .await
            .map_err(|_| unavailable())?
        {
            CheckoutOffer::Ready {
                provider,
                amount_minor,
                currency,
            } if provider == "sandbox" && Money::new(amount_minor, &currency).is_some() => {
                let price = format!(
                    "{}.{:02} {}",
                    amount_minor / 100,
                    amount_minor % 100,
                    currency.to_ascii_uppercase()
                );
                render(
                    &state,
                    &secret,
                    &Page {
                        csrf: &secret.nonce(),
                        reference: fields.get("renew"),
                        mode: "renew",
                        message: "Sandbox checkout only. No real payment will be collected.",
                        amount_minor,
                        currency: &currency,
                        price: &price,
                    },
                )
            }
            _ => message(
                &state,
                &secret,
                "Renewal checkout is currently unavailable.",
            ),
        }
    } else if fields.has("order") || fields.has("view") {
        if !fields.has("order") || !fields.has("view") {
            return Err(invalid());
        }
        match client(&state)?
            .checkout_state(&protocol::CheckoutView {
                order: fields.get("order").into(),
                view: fields.get("view").into(),
            })
            .await
            .map_err(|_| unavailable())?
        {
            CheckoutState::Missing {} => {
                Err(AppError(StatusCode::NOT_FOUND, "Checkout was not found."))
            }
            CheckoutState::Ready { provider, .. } if provider != "sandbox" => Err(unavailable()),
            CheckoutState::Ready {
                status,
                fulfilled,
                review_required,
                ..
            } => message(
                &state,
                &secret,
                match status {
                    CheckoutStatus::Pending => {
                        "Sandbox payment pending. This local checkout requires a signed event from the sandbox adapter. No real charge has been made."
                    }
                    CheckoutStatus::Paid if review_required || !fulfilled => {
                        "Payment recorded. This renewal requires operator review; the Pass has not been reactivated."
                    }
                    CheckoutStatus::Paid => "Payment Success. Your Pass has been renewed.",
                    CheckoutStatus::Declined => "Payment Declined. Your Pass has not been renewed.",
                    CheckoutStatus::Cancelled => {
                        "Payment cancelled. Your Pass has not been renewed."
                    }
                    CheckoutStatus::Refunded => {
                        "Payment refunded. The associated Pass may be on a payment hold."
                    }
                    CheckoutStatus::Disputed => {
                        "Payment disputed. The associated Pass may be on a payment hold."
                    }
                    CheckoutStatus::Expired => {
                        "Checkout expired. Open a new renewal form to try again."
                    }
                },
            ),
        }
    } else {
        render(
            &state,
            &secret,
            &Page {
                csrf: "",
                reference: "",
                mode: "intro",
                message: "",
                amount_minor: 0,
                currency: "",
                price: "",
            },
        )
    }
}
pub(crate) async fn submit(
    State(state): State<AppState>,
    Extension(RequestPeer(peer)): Extension<RequestPeer>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    if headers.get_all(header::CONTENT_TYPE).iter().count() != 1
        || headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_none_or(|v| {
                !matches!(
                    v,
                    "application/x-www-form-urlencoded"
                        | "application/x-www-form-urlencoded;charset=UTF-8"
                        | "application/x-www-form-urlencoded; charset=UTF-8"
                )
            })
    {
        return Err(invalid());
    }
    let fields = Fields::parse(
        &body,
        &["csrf", "reference", "amount_minor", "currency"],
        MAX_BODY,
    )
    .map_err(|_| invalid())?;
    let secret = CheckoutSecret::read(&headers, state.production)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if !secret.matches(fields.get("csrf")) {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "This checkout form has expired. Reload the page.",
        ));
    }
    let amount = fields
        .get("amount_minor")
        .parse::<i64>()
        .map_err(|_| invalid())?;
    if amount.to_string() != fields.get("amount_minor")
        || Money::new(amount, fields.get("currency")).is_none()
        || !reference(fields.get("reference"))
    {
        return Err(invalid());
    }
    let peer = peer
        .filter(|ip| !ip.is_unspecified() && !ip.is_multicast())
        .ok_or_else(unavailable)?;
    let result = client(&state)?
        .checkout_renewal(&protocol::CheckoutRenewal {
            reference: fields.get("reference").into(),
            nonce: secret.nonce(),
            peer,
            amount_minor: amount,
            currency: fields.get("currency").into(),
        })
        .await
        .map_err(|_| unavailable())?;
    match result {
        CheckoutCreated::Ready { order, view } => {
            // Values are encoded even though the private protocol uses canonical tokens.
            let query = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("order", &order)
                .append_pair("view", &view)
                .finish();
            let mut response = Redirect::to(&format!("/pass?{query}")).into_response();
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            );
            response.extensions_mut().insert(CheckoutPage);
            Ok(response)
        }
        CheckoutCreated::PriceChanged {} => message(
            &state,
            &secret,
            "The renewal price has changed. Reload the renewal page before continuing.",
        ),
        CheckoutCreated::Flood {} => message(
            &state,
            &secret,
            "Too many checkout attempts. Try again later.",
        ),
        CheckoutCreated::Unavailable {} => message(
            &state,
            &secret,
            "This Pass cannot currently be renewed through checkout.",
        ),
        CheckoutCreated::Capacity {} => Err(unavailable()),
    }
}

#[cfg(all(test, feature = "database-tests"))]
mod database_tests;

#[cfg(feature = "browser-tests")]
pub(crate) fn fixture_response(state: &AppState, mode: &str) -> Result<Response, String> {
    let secret = CheckoutSecret::generate().map_err(|_| "Fixture secret unavailable.")?;
    let message = match mode {
        "checkout-renew" => "Sandbox checkout only. No real payment will be collected.",
        "checkout-pending" => "Sandbox payment pending. No real charge has been made.",
        "checkout-paid" => "Payment Success. Your Pass has been renewed.",
        "checkout-declined" => "Payment Declined. Your Pass has not been renewed.",
        "checkout-cancelled" => "Payment cancelled. Your Pass has not been renewed.",
        "checkout-refunded" => "Payment refunded. The associated Pass may be on a payment hold.",
        "checkout-disputed" => "Payment disputed. The associated Pass may be on a payment hold.",
        "checkout-expired" => "Checkout expired. Open a new renewal form to try again.",
        "checkout-escaped" => "<script>owned checkout fixture</script>",
        _ => return Err("Invalid checkout fixture.".into()),
    };
    let page = Page {
        csrf: &secret.nonce(),
        reference: "owned-renewal",
        mode: if mode == "checkout-renew" {
            "renew"
        } else {
            "message"
        },
        message,
        amount_minor: 2000,
        currency: "usd",
        price: "20.00 USD",
    };
    let response = render(state, &secret, &page).map_err(|_| "Fixture rendering failed.")?;
    Ok(crate::security::verification_fixture_headers(
        response, state,
    ))
}
