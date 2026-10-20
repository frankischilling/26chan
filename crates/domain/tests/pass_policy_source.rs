use board_domain::pass_policy::{self, Login, NetworkUse, Posting, Status};
use std::net::Ipv4Addr;

#[test]
fn pass_policy_matches_isolated_source_login_and_posting_cases() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/pass-policy-cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 1120);
    for case in cases {
        let login = case["route"] == "login";
        let mask = if login { 0xffff0000 } else { 0xffffff00 };
        let previous = u32::from(
            case["last_ip"]
                .as_str()
                .unwrap()
                .parse::<Ipv4Addr>()
                .unwrap(),
        ) & mask;
        let peer = u32::from(case["peer"].as_str().unwrap().parse::<Ipv4Addr>().unwrap()) & mask;
        let use_ = NetworkUse {
            previous_known: previous != 0,
            same_scope: previous == peer,
            last_used: case["last_used"].as_i64().unwrap(),
        };
        let status = Status::try_from(case["status"].as_i64().unwrap() as i16).unwrap();
        let now = case["now"].as_i64().unwrap();
        let actual = if login {
            match pass_policy::authenticated_login(status, use_, now) {
                Login::Continue => "continue".into(),
                Login::Activate => "activate".into(),
                Login::ActivateYear => "activate_year".into(),
                Login::InUse { remaining } => format!("inuse:{remaining}"),
                Login::Disabled(_) => format!("disabled:{}", case["status"]),
            }
        } else {
            match pass_policy::authenticated_posting(
                status,
                case["expires"].as_i64().unwrap(),
                use_,
                now,
            ) {
                Posting::Continue => "continue",
                Posting::Expired => "expired",
                Posting::Disabled => "disabled",
                Posting::InUse => "inuse",
            }
            .into()
        };
        assert_eq!(actual, case["expected"].as_str().unwrap(), "{case}");
    }
}
