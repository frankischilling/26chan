//! Numeric source request scoring over complete, qualified request snapshots.
//! No HTTP extraction, posting decision, identity inference or persistent state.
//! Missing provenance must be Unavailable, never an empty request snapshot.
use super::network::ProviderValue;
use pcre2::bytes::Regex;
mod languages;

/// Complete ordered server entries, including non-HTTP entries. Header maps
/// that discard order cannot qualify this input. Parsed cookies are separate
/// from HTTP_COOKIE. The caller must establish the country/time provenance.
#[derive(Clone, Copy)]
pub struct Request<'a> {
    pub server: &'a [(&'a str, &'a [u8])],
    pub cookies: &'a [(&'a str, &'a [u8])],
    pub challenge: Option<&'a [u8]>,
    pub response: Option<&'a [u8]>,
    pub request_time: i64,
    pub country: Option<&'a [u8]>,
    pub is_op: bool,
    pub multipart: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Score(f64);
impl Score {
    pub fn value(self) -> f64 {
        self.0
    }
}

fn has(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|value| value == needle)
}
fn pos(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|value| value == needle)
}
fn truth(value: &[u8]) -> bool {
    !value.is_empty() && value != b"0"
}
fn matches(pattern: &str, value: &[u8]) -> Option<bool> {
    Regex::new(pattern).ok()?.is_match(value).ok()
}
fn capture(pattern: &str, value: &[u8]) -> Option<Option<Vec<i64>>> {
    let regex = Regex::new(pattern).ok()?;
    let Some(captures) = regex.captures(value).ok()? else {
        return Some(None);
    };
    let mut numbers = Vec::new();
    for index in 1..captures.len() {
        let bytes = captures.get(index)?.as_bytes();
        // PHP's integer cast saturates positive decimal captures at INT_MAX.
        numbers.push(bytes.iter().fold(0_i64, |n, b| {
            n.saturating_mul(10).saturating_add(i64::from(b - b'0'))
        }));
    }
    Some(Some(numbers))
}
fn valid_entries(entries: &[(&str, &[u8])]) -> bool {
    entries.len() <= 256
        && entries.iter().enumerate().all(|(i, (key, value))| {
            !key.is_empty()
                && key.len() <= 128
                && key.is_ascii()
                && value.len() <= 4096
                && !entries[..i].iter().any(|(prior, _)| prior == key)
        })
        && entries
            .iter()
            .map(|(k, v)| k.len() + v.len())
            .sum::<usize>()
            <= 65536
}
fn get<'a>(entries: &[(&str, &'a [u8])], key: &str) -> Option<&'a [u8]> {
    entries
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}
// This bounded numeric subset avoids inventing PHP results for malformed
// strings. Leading/trailing ASCII whitespace and decimal/exponent forms work.
fn numeric(value: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(value)
        .ok()?
        .trim_matches(|c: char| c.is_ascii_whitespace());
    if !matches(
        r"\A[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?\z",
        text.as_bytes(),
    )? {
        return None;
    }
    let number = text.parse::<f64>().ok()?;
    number.is_finite().then_some(number)
}
fn fast_time(now: i64, value: &[u8]) -> Option<bool> {
    let text = std::str::from_utf8(value)
        .ok()?
        .trim_matches(|c: char| c.is_ascii_whitespace());
    if let Ok(integer) = text.parse::<i64>() {
        // PHP retains integer arithmetic when both operands are integers.
        // In particular, negative values around 2^53 must not lose a second.
        return Some(i128::from(now) - i128::from(integer) == 1);
    }
    let delta = now as f64 - numeric(value)?;
    Some(delta > 0.0 && delta < 2.0)
}
fn round_two(value: f64) -> f64 {
    let mut integral = (value * 100.0).floor();
    if (integral + 1.0) / 100.0 == value {
        integral += 1.0;
    }
    if value >= (integral + 0.5) / 100.0 {
        integral += 1.0;
    }
    integral / 100.0
}

/// Invalid, oversized or incomplete snapshots do not produce a low score.
/// This does not wire the classifier into posting or reporting enforcement.
pub fn score(request: ProviderValue<Request<'_>>) -> ProviderValue<Score> {
    let ProviderValue::Available(request) = request else {
        return ProviderValue::Unavailable;
    };
    match calculate(request) {
        Some(value) if value.is_finite() => ProviderValue::Available(Score(value)),
        _ => ProviderValue::Unavailable,
    }
}
fn calculate(r: Request<'_>) -> Option<f64> {
    if !valid_entries(r.server)
        || !valid_entries(r.cookies)
        || [r.challenge, r.response, r.country]
            .into_iter()
            .flatten()
            .any(|v| v.len() > 4096)
        || r.request_time.unsigned_abs() > (1_u64 << 53)
    {
        return None;
    }
    let h = |key| get(r.server, key);
    let present = |key| h(key).is_some();
    let ua = h("HTTP_USER_AGENT").unwrap_or_default();
    let ct = h("HTTP_CONTENT_TYPE").unwrap_or_default();
    let lang = h("HTTP_ACCEPT_LANGUAGE").unwrap_or_default();
    let accept = h("HTTP_ACCEPT").unwrap_or_default();
    let referer = h("HTTP_REFERER").unwrap_or_default();
    let mobile = matches("Android|Mobile", ua)? || h("HTTP_SEC_CH_UA_MOBILE") == Some(b"?1");
    let brave = h("HTTP_SEC_CH_UA").is_some_and(|v| has(v, b"Brave")) && present("HTTP_SEC_GPC");
    let webview = has(ua, b"; wv");
    let mut app = !truth(accept) && !truth(lang) && (webview || has(referer, b"/thread/"));
    if !app && !truth(accept) && truth(lang) && has(referer, b"sys.4chan.org") {
        app = true;
    }
    if !app && !has(ua, b"Mozilla/") && matches("Android|Dalvik|iOS|iPhone", ua)? {
        app = true;
    }
    if !app
        && let Some(value) = h("HTTP_X_REQUESTED_WITH")
        && matches("floens|adamantcheese|clover", value)?
    {
        app = true;
    }
    if !app
        && matches(
            r"boundary=[0-9a-f]{8}\-[0-9a-f]{4}\-[0-9a-f]{4}\-[0-9a-f]{4}\-[0-9a-f]{12}$",
            ct,
        )?
        && (has(ua, b"Android") || (has(ua, b"Firefox/") && !truth(lang)))
    {
        app = true;
    }
    let mut add = Vec::<f64>::new();
    let mut more = Vec::<f64>::new();
    let mut webkit = false;
    let mut check_sec = false;
    if !truth(ua) {
        add.push(0.25);
    } else if (has(ua, b"Firefox/") || has(ua, b"FxiOS/")) && !has(ua, b"WebKit") {
        if r.multipart && !app && !matches(r"(?i)=-+([0-9]+|geckoformboundary[a-f0-9]+)$", ct)? {
            add.push(0.25);
        }
        if !truth(lang) {
            if !app {
                add.push(0.02);
                more.push(0.1);
            }
        } else if matches("[a-z]-[a-z]", lang)? {
            add.push(0.1);
        }
        if present("HTTP_PRAGMA") && !present("HTTP_CACHE_CONTROL") {
            add.push(0.35);
            more.push(0.1);
        }
        if has(accept, b"application/signed-exchange") {
            add.push(0.15);
        }
        if truth(accept)
            && let Some(v) = capture(r"Firefox/([0-9]+)[^0-9]", ua)?
            && !has(ua, b"PaleMoon")
        {
            let v = v[0];
            if v < 52 {
                add.push(0.2);
            } else if v < 60 {
                add.push(0.1);
            } else if v < 78 {
                add.push(0.01);
            } else if v > 500 {
                add.push(0.5);
            }
            check_sec = v > 110;
        }
    } else if has(ua, b"WebKit") {
        webkit = true;
        if r.multipart && !app {
            if pos(ct, b"WebKit").is_none_or(|p| p == 0) {
                add.push(0.25);
            } else if !has(ct, b"-") {
                add.push(0.5);
            }
        }
        if !truth(lang) {
            if !app {
                add.push(0.02);
                more.push(0.1);
            }
        } else if has(ua, b"Chrome") && !has(ua, b"Android") && matches("[a-z]-[a-z]", lang)? {
            add.push(0.1);
        }
        if let Some(v) = capture(r"Chrome/([0-9]+)[^0-9]", ua)? {
            let v = v[0];
            if v < 60 {
                add.push(0.2);
            } else if v < 70 {
                add.push(0.1);
            } else if v < 80 {
                add.push(0.05);
            } else if v > 500 {
                add.push(0.5);
            }
            check_sec = v > 110;
        }
        if let Some(v) = capture(r"Safari/([0-9]+)", ua)?
            && v[0] < 530
        {
            add.push(0.5);
        }
        if has(ua, b"iPhone") && !has(ua, b"Mobile") {
            add.push(0.06);
        }
    } else {
        if !app && r.multipart && matches("boundary=[a-zA-Z0-9]+$", ct)? {
            add.push(0.5);
        }
        if matches(
            r"Netscape/|Opera\b|Camino/|Trident/|Presto/|compatible; MSIE ",
            ua,
        )? {
            add.push(0.75);
        } else if !app && !has(ua, b"Mozilla/") {
            add.push(0.15);
            more.push(0.1);
        }
        if !app && !truth(lang) {
            more.push(0.25);
        }
        if !app && (ua.len() < 25 || !has(ua, b" ")) {
            add.push(0.25);
        }
    }
    if r.multipart && pos(ct, b"WebKit").is_some_and(|p| p > 0) && !webkit {
        add.push(0.25);
    }
    if !brave && !webview && has(ua, b"Chrome") {
        let mut started = false;
        let mut ended = false;
        for (key, _) in r.server {
            if key.starts_with("HTTP_SEC_FETCH_") {
                if started && ended {
                    add.push(0.25);
                    break;
                }
                started = true;
            } else if started {
                ended = true;
            }
        }
    }
    if let Some(v) = h("HTTP_SEC_FETCH_USER")
        && v != b"?1"
    {
        add.push(0.15);
    }
    if truth(accept) && has(accept, b"text/plain") {
        add.push(0.05);
    }
    if present("HTTP_REFERER") && !has(referer, b"4chan.org") {
        add.push(0.1);
    }
    if let Some(platform) = h("HTTP_SEC_CH_UA_PLATFORM")
        && truth(ua)
    {
        if has(ua, b"Windows") {
            if !has(platform, b"Windows") {
                add.push(0.5);
            }
        } else if has(ua, b"Mac OS") && !has(platform, b"macOS") {
            add.push(0.5);
        }
        // The source's Linux branch tests regex error === false, not no-match.
        // This fixed, bounded byte regex cannot produce that error here.
    }
    if present("HTTP_UPGRADE_INSECURE_REQUESTS") && accept == b"*/*" {
        add.push(0.2);
        more.push(0.1);
    }
    if !present("HTTP_PRAGMA") && has(ua, b"iPhone") && !has(ua, b"Safari") {
        add.push(0.09);
    }
    if has(ua, b"Windows NT ") {
        if let Some(v) = capture(r"Windows NT ([0-9]+)", ua)?
            && !has(ua, b"Mypal/")
        {
            let v = v[0];
            if v < 6 {
                add.push(if has(ua, b"Goanna") { 0.03 } else { 0.25 });
            } else if v < 10 {
                add.push(0.03);
            } else if v > 10 {
                add.push(0.5);
            }
        }
    } else if has(ua, b"Mac OS X ") {
        if let Some(v) = capture(r"Mac OS X ([0-9]+)_([0-9]+)", ua)? {
            if v[0] < 10 {
                add.push(0.5);
            } else if v[0] == 10 {
                if v[1] < 7 {
                    add.push(0.25);
                } else if v[1] < 12 {
                    add.push(0.05);
                }
            } else if has(ua, b"Safari") {
                add.push(0.3);
            }
        }
    } else if has(ua, b"Android") {
        if let Some(v) = capture(r"Android ([0-9]+)", ua)? {
            if v[0] < 4 {
                add.push(0.25);
            } else if v[0] < 8 {
                add.push(0.05);
            } else if v[0] > 20 {
                add.push(0.5);
            }
        }
        if has(ua, b"Win64;") {
            add.push(0.25);
        }
    }
    if matches("Mozilla|Firefox|Chrome", ua)?
        && !matches(
            "Windows NT|Android|Linux|Mac|iOS|X11;|BSD|Nintendo|PlayStation|Steam",
            ua,
        )?
    {
        add.push(0.2);
    }
    if matches(
        r"(?i)headless|node-fetch|python-|java/|jakarta|-perl|http-?client|-resty-|awesomium/",
        ua,
    )? {
        add.push(1.0);
    }
    if r.multipart {
        if ct == b"application/x-www-form-urlencoded" {
            add.push(0.75);
        }
    } else if !app && ct != b"application/x-www-form-urlencoded" {
        add.push(0.75);
    }
    if present("HTTP_VARY") {
        add.push(0.2);
    }
    if present("HTTP_PATH") || present("HTTP_SAME_ORIGIN") || present("HTTP_REFERRER_POLICY") {
        add.push(0.8);
    }
    if !app && !webview && h("HTTP_SEC_FETCH_MODE") == Some(b"navigate") {
        if !r.is_op && !mobile {
            add.push(0.02);
            more.push(0.1);
        }
        if present("HTTP_SEC_CH_UA_MODEL") && !present("HTTP_SEC_CH_UA_BITNESS") {
            add.push(0.1);
            more.push(0.2);
        }
    }
    if pos(ua, b"like Mac OS").is_some_and(|p| p > 0) && h("HTTP_SEC_FETCH_SITE") == Some(b"none") {
        add.push(0.08);
        more.push(0.1);
    }
    if !app
        && (!h("HTTP_COOKIE").is_some_and(truth)
            || (r.cookies.len() == 1 && get(r.cookies, "cf_clearance").is_some()))
    {
        add.push(0.05);
        more.push(0.25);
    }
    if let Some(tcs) = get(r.cookies, "_tcs") {
        let fields: Vec<_> = tcs.split(|b| *b == b'.').take(4).collect();
        let tz = fields.get(1).copied().unwrap_or_default();
        let time = fields.get(2).copied().unwrap_or_default();
        let version = fields.get(3).copied().unwrap_or_default();
        if !truth(version) || (!webview && has(ua, b"Chrome/") && numeric(version) != Some(33.0)) {
            add.push(0.09);
        }
        if truth(time)
            && r.challenge.is_some_and(|v| v != b"noop")
            && r.response.is_some_and(truth)
            && fast_time(r.request_time, time)?
        {
            add.push(1.0);
        }
        if let Some(header_tz) = h("HTTP_X_TIMEZONE") {
            let tz0 = tz.split(|b| *b == b'/').next().unwrap_or_default();
            if truth(tz0) {
                if tz0 == b"UTC" {
                    add.push(0.02);
                    more.push(0.02);
                } else if tz0 == b"Etc" {
                    add.push(0.01);
                    more.push(0.01);
                } else if !header_tz.starts_with(tz0)
                    && (!has(tz0, b"Atlantic") || !has(header_tz, b"Europe"))
                {
                    add.push(0.03);
                    more.push(0.03);
                }
            }
        }
    }
    if !truth(accept) && !app {
        add.push(0.15);
    }
    if check_sec
        && !app
        && (!present("HTTP_SEC_FETCH_DEST")
            || !present("HTTP_SEC_FETCH_MODE")
            || !present("HTTP_SEC_FETCH_SITE"))
    {
        add.push(0.15);
    }
    if h("HTTP_X_HTTP_VERSION") == Some(b"HTTP/1.1") {
        more.push(0.1);
    }
    let model = r
        .server
        .iter()
        .position(|(k, _)| *k == "HTTP_SEC_CH_UA_MODEL");
    let platform = r
        .server
        .iter()
        .position(|(k, _)| *k == "HTTP_SEC_CH_UA_PLATFORM");
    if let (Some(a), Some(b)) = (model, platform)
        && a.abs_diff(b) > 5
    {
        add.push(0.05);
        more.push(0.01);
    }
    if let Some(country) = r.country.filter(|v| truth(v)) {
        if truth(lang) && !lang.starts_with(b"en") {
            if let Some(pattern) = languages::languages(country)
                && !matches(&format!(r"\b(?:{pattern})\b"), lang)?
            {
                more.push(0.025);
                add.push(0.025);
            }
            if !matches("iPhone|iPad", ua)? && !has(lang, b";") {
                more.push(0.025);
                add.push(0.025);
            }
        }
        const GROUP_ZERO: &str = "AD AE AF AG AI AL AM AN AO AQ AS AW AX AZ BB BD BF BH BI BJ BL BM BN BO BQ BS BT BV BW BZ CC CD CF CG CI CK CM CN CR CU CV CW CX DJ DM DO DZ EC EG EH ER ET FJ FK FM FO GA GD GF GG GH GI GM GN GP GQ GS GT GU GW GY HK HM HN HT IM IO IQ IR JE JM JO KE KG KH KI KM KN KP KW KY KZ LA LB LC LK LR LS LY MA MD MF MG MH ML MM MN MO MP MQ MR MS MU MV MW MZ NA NC NE NF NG NI NP NR NU OM PA PF PG PK PM PN PS PW PY QA RE RW SA SB SC SD SH SJ SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TJ TK TM TN TO TP TR TT TV TZ UG UM UZ VA VC VG VI VU WF WS YE YT YU ZM ZW XX";
        const GROUP_ONE: &str = "BR VE AR CL UY CO PE MX UA BA RU MC MK CY MT ME KR JP TH VN ID";
        if GROUP_ZERO.split(' ').any(|v| v.as_bytes() == country) {
            more.push(0.3);
        } else if GROUP_ONE.split(' ').any(|v| v.as_bytes() == country) {
            more.push(0.1);
        }
    }
    let mut total: f64 = add.iter().sum();
    if total > 0.0 {
        for value in more {
            total *= 1.0 + value;
        }
    }
    Some(round_two(total))
}

#[cfg(test)]
mod tests;
