//! Dormant emote-event rules. No endpoint activation or client-selected lottery
//! is provided. A persistent caller must authorize the actor and lock its state.
use crate::ValidationError;

#[rustfmt::skip]
mod data;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Unicode,
    Image,
    RareImage,
}
impl Kind {
    pub fn source_number(self) -> u8 {
        match self {
            Self::Unicode => 1,
            Self::Image => 2,
            Self::RareImage => 3,
        }
    }
    pub fn price(self) -> i64 {
        match self {
            Self::Unicode => 50,
            Self::Image => 150,
            Self::RareImage => 300,
        }
    }
    fn refund(self) -> i64 {
        if self == Self::RareImage { 10 } else { 5 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Emote {
    name: &'static str,
    kind: Kind,
    value: &'static str,
    width: u16,
    height: u16,
}
impl Emote {
    pub fn name(&self) -> &'static str {
        self.name
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }
    pub fn value(&self) -> &'static str {
        self.value
    }
    pub fn dimensions(&self) -> Option<(u16, u16)> {
        (self.kind != Kind::Unicode).then_some((self.width, self.height))
    }
    pub fn id(&self) -> &'static str {
        if self.kind == Kind::Unicode {
            self.name
        } else {
            self.value
        }
    }
    pub fn source_tuple(&self) -> (&'static str, u8, &'static str) {
        (self.name, self.kind.source_number(), self.value)
    }
}

pub fn roster() -> &'static [Emote] {
    data::EMOTES
}
pub fn lookup(id: &str) -> Option<&'static Emote> {
    (id.len() <= 64)
        .then(|| data::EMOTES.iter().find(|emote| emote.id() == id))
        .flatten()
}
/// The source uses 50%, 40%, 10% pools, followed by uniform selection within
/// that pool. The caller must supply a server-generated draw, never a form field.
pub fn pool(draw: u8) -> Option<&'static [&'static str]> {
    let index = match draw {
        0..=49 => 0,
        50..=89 => 1,
        90..=99 => 2,
        _ => return None,
    };
    Some(data::POOLS[index])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    start_ts: i64,
    balance: i64,
    owned: Vec<usize>,
}
pub struct Acquisition {
    pub account: Account,
    pub obtained: Vec<&'static Emote>,
}
impl Account {
    pub fn new(now: i64, established_session: bool) -> Result<Self, ValidationError> {
        Self::from_stored(now, if established_session { 100 } else { 0 }, &[])
    }
    pub fn from_stored(
        start_ts: i64,
        balance: i64,
        owned: &[String],
    ) -> Result<Self, ValidationError> {
        if start_ts < 0 || owned.len() > data::EMOTES.len() {
            return Err(ValidationError("Invalid event account"));
        }
        let mut indices = Vec::with_capacity(owned.len());
        for name in owned {
            let index = data::EMOTES
                .iter()
                .position(|emote| emote.name == name)
                .ok_or(ValidationError("Invalid event account"))?;
            if indices.contains(&index) {
                return Err(ValidationError("Invalid event account"));
            }
            indices.push(index);
        }
        Ok(Self {
            start_ts,
            balance,
            owned: indices,
        })
    }
    pub fn start_ts(&self) -> i64 {
        self.start_ts
    }
    /// Stored balance excludes the elapsed-time credit, as in source JSON.
    pub fn balance(&self) -> i64 {
        self.balance
    }
    pub fn owned(&self) -> impl Iterator<Item = &'static Emote> + '_ {
        self.owned.iter().map(|index| &data::EMOTES[*index])
    }
    pub fn full_balance(&self, now: i64) -> Result<i64, ValidationError> {
        let elapsed = now
            .checked_sub(self.start_ts)
            .filter(|value| *value >= 0)
            .ok_or(ValidationError("Invalid event time"))?;
        (elapsed / 300)
            .checked_mul(20)
            .and_then(|credit| credit.checked_add(self.balance))
            .ok_or(ValidationError("Invalid event balance"))
    }
    pub fn buy(&self, id: &str, now: i64) -> Result<Acquisition, ValidationError> {
        let emote = lookup(id).ok_or(ValidationError("Bad Request"))?;
        let index = data::EMOTES
            .iter()
            .position(|item| item.name == emote.name)
            .expect("release roster");
        if self.owned.contains(&index) {
            return Err(ValidationError("You already have that emote"));
        }
        let cost = emote.kind.price();
        if self.full_balance(now)? < cost {
            return Err(ValidationError("Not enough points"));
        }
        let mut next = self.clone();
        next.balance = next
            .balance
            .checked_sub(cost)
            .ok_or(ValidationError("Invalid event balance"))?;
        next.owned.push(index);
        Ok(Acquisition {
            account: next,
            obtained: vec![emote],
        })
    }
    /// `selected` must come from the server's bounded random draws. It is an
    /// explicit input so source fixtures can replay the same outcomes. Validate
    /// the full charge before refunds, including duplicates within one roll.
    pub fn apply_roll(&self, selected: &[&str], now: i64) -> Result<Acquisition, ValidationError> {
        if !matches!(selected.len(), 3 | 10) {
            return Err(ValidationError("Bad Request"));
        }
        let cost = (selected.len() as i64) * 10;
        let obtained: Vec<_> = selected
            .iter()
            .map(|id| lookup(id).ok_or(ValidationError("Bad Request")))
            .collect::<Result<_, _>>()?;
        if self.full_balance(now)? < cost {
            return Err(ValidationError("Not enough points"));
        }
        let mut next = self.clone();
        let mut refund = 0;
        for emote in &obtained {
            let index = data::EMOTES
                .iter()
                .position(|item| item.name == emote.name)
                .expect("release roster");
            if next.owned.contains(&index) {
                refund += emote.kind.refund();
            } else {
                next.owned.push(index);
            }
        }
        next.balance = next
            .balance
            .checked_add(refund)
            .and_then(|balance| balance.checked_sub(cost))
            .ok_or(ValidationError("Invalid event balance"))?;
        Ok(Acquisition {
            account: next,
            obtained,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_roster_and_balances_match_executed_source() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/event-emote-reference.json"
        ))
        .unwrap();
        assert_eq!(roster().len(), 166);
        for row in fixture["lookups"].as_array().unwrap() {
            assert_eq!(
                serde_json::to_value(lookup(row["id"].as_str().unwrap()).map(Emote::source_tuple))
                    .unwrap(),
                row["expected"]
            );
        }
        for (name, meta) in fixture["emotes"].as_object().unwrap() {
            let emote = roster().iter().find(|emote| emote.name() == name).unwrap();
            assert_eq!(
                emote.kind().source_number(),
                meta[0].as_u64().unwrap() as u8
            );
            assert_eq!(emote.value(), meta[1]);
            if meta[0] != 1 {
                assert_eq!(
                    emote.dimensions(),
                    Some((
                        meta[2].as_u64().unwrap() as u16,
                        meta[3].as_u64().unwrap() as u16
                    ))
                );
            }
        }
        for row in fixture["balances"].as_array().unwrap() {
            let account = Account::from_stored(
                row["start_ts"].as_i64().unwrap(),
                row["balance"].as_i64().unwrap(),
                &[],
            )
            .unwrap();
            assert_eq!(
                account.full_balance(row["now"].as_i64().unwrap()).unwrap(),
                row["expected"].as_i64().unwrap()
            );
        }
        for draw in 0..100 {
            let index = if draw < 50 {
                0
            } else if draw < 90 {
                1
            } else {
                2
            };
            assert_eq!(
                serde_json::to_value(pool(draw).unwrap()).unwrap(),
                fixture["pools"][index]
            );
        }
        assert!(pool(100).is_none());
    }
    #[test]
    fn buys_and_recorded_rolls_match_the_dormant_source_ledger() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/event-emote-ledger-reference.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let stored = &case["account"];
            let owned: Vec<String> = serde_json::from_value(stored["owned"].clone()).unwrap();
            let account = Account::from_stored(
                stored["start_ts"].as_i64().unwrap(),
                stored["balance"].as_i64().unwrap(),
                &owned,
            )
            .unwrap();
            let expected = &case["expected"];
            let now = case["now"].as_i64().unwrap();
            let result = if case["action"] == "buy" {
                account.buy(case["id"].as_str().unwrap(), now)
            } else {
                let selected: Vec<&str> =
                    if let Some(obtained) = expected["response"]["data"]["obtained"].as_array() {
                        obtained
                            .iter()
                            .map(|row| {
                                if row[1] == 1 {
                                    row[0].as_str().unwrap()
                                } else {
                                    row[2].as_str().unwrap()
                                }
                            })
                            .collect()
                    } else {
                        // Rejected source rolls have no visible draw. A valid
                        // fixed draw must still fail the same pre-refund charge.
                        vec!["happy"; case["size"].as_u64().unwrap() as usize]
                    };
                account.apply_roll(&selected, now)
            };
            match result {
                Ok(acquired) => {
                    assert_eq!(expected["response"]["status"], "success", "{case}");
                    assert_eq!(expected["writes"], 1);
                    assert_eq!(
                        acquired.account.balance(),
                        expected["account"]["balance"].as_i64().unwrap()
                    );
                    assert_eq!(
                        acquired.account.start_ts(),
                        expected["account"]["start_ts"].as_i64().unwrap()
                    );
                    let names: Vec<_> = acquired.account.owned().map(Emote::name).collect();
                    assert_eq!(
                        serde_json::to_value(names).unwrap(),
                        expected["account"]["owned"]
                    );
                    let obtained = if case["action"] == "buy" {
                        serde_json::to_value(acquired.obtained[0].source_tuple()).unwrap()
                    } else {
                        serde_json::to_value(
                            acquired
                                .obtained
                                .iter()
                                .map(|emote| emote.source_tuple())
                                .collect::<Vec<_>>(),
                        )
                        .unwrap()
                    };
                    assert_eq!(obtained, expected["response"]["data"]["obtained"], "{case}");
                    assert_eq!(
                        acquired.account.balance(),
                        expected["response"]["data"]["balance"].as_i64().unwrap()
                    );
                }
                Err(error) => {
                    assert_eq!(expected["response"]["status"], "error", "{case}");
                    assert_eq!(
                        error.0,
                        expected["response"]["msg"].as_str().unwrap(),
                        "{case}"
                    );
                    assert_eq!(expected["writes"], 0);
                    if let Some(points) = expected["response"]["pts"].as_i64() {
                        assert_eq!(account.balance(), points);
                    }
                }
            }
        }
    }

    #[test]
    fn duplicate_recycling_includes_new_items_earlier_in_the_same_roll() {
        let account = Account::new(1000, true).unwrap();
        let result = account
            .apply_roll(&["happy", "happy", "happy"], 1000)
            .unwrap();
        assert_eq!(result.account.balance(), 80);
        assert_eq!(result.account.owned().count(), 1);
        assert_eq!(result.obtained.len(), 3);
        let rare = pool(99).unwrap()[0];
        let result = account.apply_roll(&[rare, rare, rare], 1000).unwrap();
        assert_eq!(result.account.balance(), 90);
        assert_eq!(result.account.owned().count(), 1);
        assert_eq!(account.balance(), 100);
        assert_eq!(account.owned().count(), 0);
    }
    #[test]
    fn spending_uses_tick_credit_without_resetting_the_source_ledger() {
        let account = Account::new(1000, false).unwrap();
        assert!(account.buy("happy", 1599).is_err());
        let bought = account.buy("happy", 1900).unwrap().account;
        assert_eq!(bought.balance(), -50);
        assert_eq!(bought.full_balance(1900).unwrap(), 10);
        assert_eq!(bought.start_ts(), 1000);
        assert_eq!(
            bought.buy("happy", 1900).err().unwrap().0,
            "You already have that emote"
        );
        assert!(
            account
                .apply_roll(&["happy", "happy", "happy"], 1299)
                .is_err()
        );
        assert!(account.full_balance(999).is_err());
        assert!(account.apply_roll(&["happy"], 1900).is_err());
    }
    #[test]
    fn stored_or_selected_unknown_values_never_become_markup_or_credit() {
        assert!(Account::from_stored(0, 0, &["<img>".into()]).is_err());
        assert!(Account::from_stored(0, 0, &["happy".into(), "happy".into()]).is_err());
        let account = Account::new(0, true).unwrap();
        assert!(account.apply_roll(&["happy", "happy", "../x"], 0).is_err());
        assert!(
            Account::from_stored(0, i64::MAX, &[])
                .unwrap()
                .full_balance(300)
                .is_err()
        );
        assert!(Account::new(-1, true).is_err());
        for emote in roster() {
            if emote.kind() == Kind::Unicode {
                assert!(emote.value().split(';').filter(|v| !v.is_empty()).all(|v| {
                    v.strip_prefix("&#x")
                        .is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()))
                }));
            } else {
                assert_eq!(emote.value().len(), 8);
                assert!(emote.value().bytes().all(|b| b.is_ascii_hexdigit()));
            }
        }
    }
}
