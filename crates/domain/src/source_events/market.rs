//! Fictional event balances only. A caller must authorize the actor, lock its
//! stored account and persist the returned state in one transaction. This pure
//! model never activates the source's unconditionally closed market endpoint.
use super::STOCKS;
use crate::{ValidationError, php_scalar};
use std::collections::BTreeMap;

pub const STARTING_AMOUNT: i64 = 1000;
pub const MAX_ORDER: i64 = 1000;
pub const COOLDOWN_SECONDS: i64 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Order {
    side: Side,
    stock: usize,
    amount: i64,
    price: i64,
}

impl Order {
    pub fn parse(
        side: Side,
        stock: &str,
        amount: &str,
        price: &str,
    ) -> Result<Self, ValidationError> {
        if stock.is_empty() || matches!(stock, "_" | "0") {
            return Err(ValidationError("Stock not found"));
        }
        if amount.len() > 100 {
            return Err(ValidationError("Invalid amount"));
        }
        let amount = php_scalar::integer(amount);
        if !(1..=MAX_ORDER).contains(&amount) {
            return Err(ValidationError("Invalid amount"));
        }
        if price.len() > 100 {
            return Err(ValidationError("Invalid price"));
        }
        let price = php_scalar::integer(price);
        if price <= 0 {
            return Err(ValidationError("Invalid price"));
        }
        // Unknown symbols have no source price. Do not let an operator quote
        // create a new tradable symbol outside the captured public list.
        let stock = STOCKS
            .iter()
            .position(|candidate| *candidate == stock)
            .ok_or(ValidationError("Invalid price"))?;
        Ok(Self {
            side,
            stock,
            amount,
            price,
        })
    }

    pub fn stock(&self) -> &'static str {
        STOCKS[self.stock]
    }
    pub fn amount(&self) -> i64 {
        self.amount
    }
    pub fn price(&self) -> i64 {
        self.price
    }
    pub fn side(&self) -> Side {
        self.side
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    balance: i64,
    holdings: [i64; 18],
    last_order_at: i64,
}

impl Account {
    pub fn new(now: i64) -> Result<Self, ValidationError> {
        Self::from_stored(STARTING_AMOUNT, [0; 18], now)
    }

    pub fn from_stored(
        balance: i64,
        holdings: [i64; 18],
        last_order_at: i64,
    ) -> Result<Self, ValidationError> {
        if balance < 0 || holdings.iter().any(|count| *count < 0) || last_order_at < 0 {
            return Err(ValidationError("Invalid event account"));
        }
        Ok(Self {
            balance,
            holdings,
            last_order_at,
        })
    }

    /// Apply no partial updates. The stored account and selected quote must be
    /// locked by the caller; stale prices fail before funds/cooldown checks.
    pub fn trade(
        &self,
        order: Order,
        current_price: i64,
        now: i64,
    ) -> Result<Self, ValidationError> {
        if current_price <= 0 {
            return Err(ValidationError("Invalid price"));
        }
        if current_price != order.price {
            return Err(ValidationError("The price has changed."));
        }
        let cost = order
            .amount
            .checked_mul(current_price)
            .ok_or(ValidationError("Invalid amount"))?;
        let quantity = self.holdings[order.stock];
        if (order.side == Side::Buy && self.balance < cost)
            || (order.side == Side::Sell && quantity < order.amount)
        {
            return Err(ValidationError("Your account balance is too low"));
        }
        if now
            .checked_sub(self.last_order_at)
            .is_none_or(|elapsed| elapsed < COOLDOWN_SECONDS)
        {
            return Err(ValidationError(
                "You can only make an order once every 10 seconds",
            ));
        }
        let mut next = self.clone();
        match order.side {
            Side::Buy => {
                next.balance -= cost;
                next.holdings[order.stock] = quantity
                    .checked_add(order.amount)
                    .ok_or(ValidationError("Invalid amount"))?;
            }
            Side::Sell => {
                next.balance = self
                    .balance
                    .checked_add(cost)
                    .ok_or(ValidationError("Invalid amount"))?;
                next.holdings[order.stock] -= order.amount;
            }
        }
        next.last_order_at = now;
        Ok(next)
    }

    pub fn balance(&self) -> i64 {
        self.balance
    }
    pub fn holdings(&self) -> &[i64; 18] {
        &self.holdings
    }
    pub fn last_order_at(&self) -> i64 {
        self.last_order_at
    }

    /// Source JSON omits zero holdings; private actor/session data is not part
    /// of this value and cannot accidentally enter public serialization.
    pub fn public_balance(&self) -> BTreeMap<&'static str, i64> {
        let mut result = BTreeMap::from([("balance", self.balance)]);
        for (stock, quantity) in STOCKS.into_iter().zip(self.holdings) {
            if quantity > 0 {
                result.insert(stock, quantity);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_match_executed_dormant_source_controller_cases() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/event-market-reference.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let fields = &case["fields"];
            let mut holdings = [0; 18];
            let stock = fields["stock"].as_str().unwrap();
            let index = STOCKS.iter().position(|candidate| *candidate == stock);
            if let Some(index) = index {
                holdings[index] = case["holding"].as_i64().unwrap();
            }
            let account = Account::from_stored(
                case["balance"].as_i64().unwrap(),
                holdings,
                case["last"].as_i64().unwrap(),
            )
            .unwrap();
            let side = if case["side"] == "buy" {
                Side::Buy
            } else {
                Side::Sell
            };
            let result = Order::parse(
                side,
                stock,
                fields["amount"].as_str().unwrap(),
                fields["price"].as_str().unwrap(),
            )
            .and_then(|order| {
                account.trade(
                    order,
                    case["current_price"].as_i64().unwrap(),
                    case["now"].as_i64().unwrap(),
                )
            });
            let expected = &case["expected"];
            match result {
                Ok(next) => {
                    assert_eq!(
                        serde_json::to_value(next.public_balance()).unwrap(),
                        expected["response"],
                        "{case}"
                    );
                    assert_eq!(next.balance(), expected["balance"].as_i64().unwrap());
                    assert_eq!(
                        next.holdings()[index.unwrap()],
                        expected["holding"].as_i64().unwrap()
                    );
                    assert_eq!(next.last_order_at(), expected["last"].as_i64().unwrap());
                    assert_eq!(expected["writes"], 2);
                }
                Err(error) => {
                    assert_eq!(
                        error.0,
                        expected["response"]["error"].as_str().unwrap(),
                        "{case}"
                    );
                    assert_eq!(expected["writes"], 0);
                }
            }
        }
    }

    #[test]
    fn every_source_stock_can_be_bought_and_sold_without_partial_state() {
        for stock in STOCKS {
            let account = Account::new(100).unwrap();
            let order = Order::parse(Side::Buy, stock, "2", "100").unwrap();
            assert!(account.trade(order, 100, 109).is_err());
            let bought = account.trade(order, 100, 110).unwrap();
            assert_eq!(
                bought.public_balance(),
                BTreeMap::from([("balance", 800), (stock, 2)])
            );
            assert_eq!(
                account.public_balance(),
                BTreeMap::from([("balance", 1000)])
            );
            let sold = bought
                .trade(
                    Order::parse(Side::Sell, stock, "2", "150").unwrap(),
                    150,
                    120,
                )
                .unwrap();
            assert_eq!(sold.public_balance(), BTreeMap::from([("balance", 1100)]));
            assert_eq!(sold.last_order_at(), 120);
        }
    }

    #[test]
    fn source_scalar_coercions_and_error_precedence_remain_explicit() {
        assert_eq!(
            Order::parse(Side::Buy, "PEPE", "1e3", "10garbage")
                .unwrap()
                .amount(),
            1000
        );
        assert_eq!(
            Order::parse(Side::Buy, "PEPE", "2.9", "100")
                .unwrap()
                .amount(),
            2
        );
        for stock in ["", "_"] {
            assert_eq!(
                Order::parse(Side::Buy, stock, "0", "0").unwrap_err().0,
                "Stock not found"
            );
        }
        for stock in ["pepe", "UNKNOWN", "PEPE\0"] {
            assert_eq!(
                Order::parse(Side::Buy, stock, "1", "1").unwrap_err().0,
                "Invalid price"
            );
            assert_eq!(
                Order::parse(Side::Buy, stock, "0", "0").unwrap_err().0,
                "Invalid amount"
            );
        }
        assert_eq!(
            Order::parse(Side::Buy, "PEPE", "1001", "0").unwrap_err().0,
            "Invalid amount"
        );
        assert_eq!(
            Order::parse(Side::Buy, "PEPE", "1", "0").unwrap_err().0,
            "Invalid price"
        );
        let account = Account::new(100).unwrap();
        let order = Order::parse(Side::Buy, "PEPE", "1000", "100").unwrap();
        assert_eq!(
            account.trade(order, 101, 100).unwrap_err().0,
            "The price has changed."
        );
        assert_eq!(
            account.trade(order, 100, 100).unwrap_err().0,
            "Your account balance is too low"
        );
    }

    #[test]
    fn overflow_future_clocks_and_overselling_cannot_create_credit() {
        let mut holdings = [0; 18];
        holdings[0] = i64::MAX;
        let account = Account::from_stored(i64::MAX, holdings, 100).unwrap();
        let buy = Order::parse(Side::Buy, "PEPE", "1", "1").unwrap();
        assert!(account.trade(buy, 1, 99).is_err());
        assert!(account.trade(buy, 1, 110).is_err());
        let sell = Order::parse(Side::Sell, "PEPE", "1", "1").unwrap();
        assert!(account.trade(sell, 1, 110).is_err());
        assert_eq!(account.balance(), i64::MAX);
        let huge = Order::parse(Side::Buy, "PEPE", "1000", &i64::MAX.to_string()).unwrap();
        assert!(account.trade(huge, i64::MAX, 110).is_err());
        assert!(Account::new(100).unwrap().trade(sell, 1, 110).is_err());
        assert!(Account::from_stored(-1, [0; 18], 0).is_err());
    }
}
