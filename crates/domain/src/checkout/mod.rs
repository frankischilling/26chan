//! Native checkout rules. Missing legacy provider controllers cannot establish
//! price defaults or callback trust; deployment policy supplies those values.
pub mod signature;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Product {
    PassPurchase,
    PassRenewal,
    Donation,
    BoardAdvertisement,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Pending,
    Paid,
    Declined,
    Cancelled,
    Refunded,
    Disputed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Paid,
    Declined,
    Cancelled,
    FullRefund,
    Disputed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    Apply(State),
    Duplicate,
    Refuse,
}
impl State {
    pub fn apply(self, event: Event) -> Transition {
        use Event as E;
        use State as S;
        match (self, event) {
            (S::Pending, E::Paid) => Transition::Apply(S::Paid),
            (S::Pending, E::Declined) => Transition::Apply(S::Declined),
            (S::Pending, E::Cancelled) => Transition::Apply(S::Cancelled),
            (S::Pending | S::Paid, E::FullRefund) => Transition::Apply(S::Refunded),
            (S::Pending | S::Paid, E::Disputed) => Transition::Apply(S::Disputed),
            (S::Paid, E::Paid)
            | (S::Declined, E::Declined)
            | (S::Cancelled, E::Cancelled)
            | (S::Refunded, E::FullRefund)
            | (S::Disputed, E::Disputed) => Transition::Duplicate,
            _ => Transition::Refuse,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Money {
    minor: i64,
    currency: [u8; 3],
}
impl Money {
    pub fn new(minor: i64, currency: &str) -> Option<Self> {
        if !(1..=100_000_000).contains(&minor)
            || currency.len() != 3
            || !currency.bytes().all(|b| b.is_ascii_lowercase())
        {
            return None;
        }
        Some(Self {
            minor,
            currency: currency.as_bytes().try_into().ok()?,
        })
    }
    pub fn minor(self) -> i64 {
        self.minor
    }
    pub fn currency(&self) -> &str {
        std::str::from_utf8(&self.currency).expect("ASCII currency")
    }
}
/// The order's provider binding and product are fixed before any notification.
pub struct Binding<'a> {
    pub order: &'a str,
    pub provider_object: &'a str,
    pub product: Product,
    pub amount: Money,
    pub created: i64,
    pub expires: i64,
}
impl Binding<'_> {
    pub fn matches_capture(&self, event: &Binding<'_>, occurred: i64) -> bool {
        self.created >= 0
            && self.expires > self.created
            && self.expires <= 253_402_300_799
            && occurred >= self.created
            && occurred <= self.expires
            && self.order == event.order
            && self.provider_object == event.provider_object
            && self.product == event.product
            && self.amount == event.amount
            && !self.order.is_empty()
            && self.order.len() <= 128
            && !self.provider_object.is_empty()
            && self.provider_object.len() <= 255
            && [self.order, self.provider_object].iter().all(|v| {
                v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
    }
}
/// Source-compatible public identifier; authentication still requires its PIN.
pub fn new_pass_identifier() -> Result<String, ring::error::Unspecified> {
    use ring::rand::{SecureRandom, SystemRandom};
    const ALPHABET: &[u8; 36] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut random = [0; 64];
    SystemRandom::new().fill(&mut random)?;
    let text: String = random
        .into_iter()
        .filter(|byte| *byte < 252)
        .take(10)
        .map(|byte| char::from(ALPHABET[usize::from(byte % 36)]))
        .collect();
    if text.len() == 10 {
        Ok(text)
    } else {
        Err(ring::error::Unspecified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkout_capability_retries_are_stable_and_purpose_separated() {
        use crate::challenge_secrets::{
            CheckoutToken, PassRecoveryToken, PassSessionToken, VerificationKey,
        };
        let key = VerificationKey::parse(&"11".repeat(32)).unwrap();
        let nonce = "ab".repeat(32);
        let first = key.checkout_view(&nonce, &[1; 32]).unwrap();
        let retry = key.checkout_view(&nonce, &[1; 32]).unwrap();
        assert_eq!(first.storage_hash(), retry.storage_hash());
        assert_ne!(
            first.storage_hash(),
            key.checkout_view(&nonce, &[2; 32]).unwrap().storage_hash()
        );
        assert_ne!(
            first.storage_hash(),
            key.checkout_view(&"ac".repeat(32), &[1; 32])
                .unwrap()
                .storage_hash()
        );
        assert!(CheckoutToken::parse(&first.encode()).is_some());
        assert!(PassSessionToken::parse(&first.encode()).is_none());
        assert!(PassRecoveryToken::parse(&first.encode()).is_none());
        assert!(key.checkout_view("wrong", &[1; 32]).is_none());
        let purchase = key.checkout_purchase_view(&nonce, &[1; 32]).unwrap();
        assert_ne!(purchase.storage_hash(), first.storage_hash());
        assert_eq!(
            purchase.storage_hash(),
            key.checkout_purchase_view(&nonce, &[1; 32])
                .unwrap()
                .storage_hash()
        );
        assert_ne!(
            purchase.storage_hash(),
            key.checkout_purchase_view(&nonce, &[2; 32])
                .unwrap()
                .storage_hash()
        );
        assert!(key.checkout_purchase_view("wrong", &[1; 32]).is_none());
    }

    #[test]
    fn purchase_identifiers_use_the_public_ten_character_alphabet() {
        for _ in 0..64 {
            let identifier = new_pass_identifier().unwrap();
            assert_eq!(identifier.len(), 10);
            assert!(
                identifier
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            );
        }
    }

    #[test]
    fn terminal_states_cannot_be_reactivated_by_old_success() {
        for state in [
            State::Declined,
            State::Cancelled,
            State::Refunded,
            State::Disputed,
        ] {
            assert_eq!(state.apply(Event::Paid), Transition::Refuse);
        }
        assert_eq!(
            State::Pending.apply(Event::Paid),
            Transition::Apply(State::Paid)
        );
        assert_eq!(State::Paid.apply(Event::Paid), Transition::Duplicate);
        assert_eq!(
            State::Paid.apply(Event::FullRefund),
            Transition::Apply(State::Refunded)
        );
        assert_eq!(
            State::Refunded.apply(Event::FullRefund),
            Transition::Duplicate
        );
        assert_eq!(
            State::Pending.apply(Event::FullRefund),
            Transition::Apply(State::Refunded)
        );
    }
    #[test]
    fn provider_product_money_and_capture_window_are_bound() {
        let order = Binding {
            order: "owned-order",
            provider_object: "cs_owned",
            product: Product::PassPurchase,
            amount: Money::new(2000, "usd").unwrap(),
            created: 100,
            expires: 200,
        };
        let mut event = Binding {
            order: "owned-order",
            provider_object: "cs_owned",
            product: Product::PassPurchase,
            amount: Money::new(2000, "usd").unwrap(),
            created: 0,
            expires: 0,
        };
        for time in [100, 150, 200] {
            assert!(order.matches_capture(&event, time));
        }
        for time in [99, 201, i64::MAX] {
            assert!(!order.matches_capture(&event, time));
        }
        event.product = Product::Donation;
        assert!(!order.matches_capture(&event, 150));
        event.product = Product::PassPurchase;
        event.amount = Money::new(2000, "eur").unwrap();
        assert!(!order.matches_capture(&event, 150));
        event.amount = Money::new(1999, "usd").unwrap();
        assert!(!order.matches_capture(&event, 150));
        event.amount = order.amount;
        event.provider_object = "cs_other";
        assert!(!order.matches_capture(&event, 150));
        for amount in [0, -1, 100_000_001, i64::MAX] {
            assert!(Money::new(amount, "usd").is_none());
        }
        for currency in ["USD", "u$d", "usd\n", "us", ""] {
            assert!(Money::new(1, currency).is_none());
        }
    }
}
