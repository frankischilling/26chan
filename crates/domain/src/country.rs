//! Offline, operator-supplied country data. Never accepts a browser country hint.
use std::{io::Read, net::IpAddr, path::Path};

const MAX_DATABASE_BYTES: u64 = 64 * 1024 * 1024;

pub struct CountryDatabase(maxminddb::Reader<Vec<u8>>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Country {
    pub code: String,
    pub name: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CountryError(pub &'static str);

impl Country {
    fn unknown() -> Self {
        Self {
            code: "XX".into(),
            name: "Unknown".into(),
        }
    }
}

impl CountryDatabase {
    /// Load once into safe Rust memory. The path and lookup address never enter errors.
    pub fn load(path: &Path) -> Result<Self, CountryError> {
        if !path.is_absolute() {
            return Err(CountryError(
                "COUNTRY_DATABASE must be an absolute file path.",
            ));
        }
        let before = std::fs::metadata(path)
            .map_err(|_| CountryError("Country database could not be inspected."))?;
        // Reject devices and FIFOs before open; even operator configuration
        // mistakes must not turn a country source into a blocking stream.
        if !before.is_file() || before.len() == 0 || before.len() > MAX_DATABASE_BYTES {
            return Err(CountryError(
                "Country database must be a regular file of at most 64 MiB.",
            ));
        }
        let file = std::fs::File::open(path)
            .map_err(|_| CountryError("Country database could not be opened."))?;
        let metadata = file
            .metadata()
            .map_err(|_| CountryError("Country database could not be inspected."))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_DATABASE_BYTES {
            return Err(CountryError(
                "Country database must be a regular file of at most 64 MiB.",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_DATABASE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| CountryError("Country database could not be read."))?;
        if bytes.len() as u64 > MAX_DATABASE_BYTES {
            return Err(CountryError("Country database exceeds 64 MiB."));
        }
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, CountryError> {
        if bytes.is_empty() || bytes.len() as u64 > MAX_DATABASE_BYTES {
            return Err(CountryError("Country database size is invalid."));
        }
        let reader = maxminddb::Reader::from_source(bytes)
            .map_err(|_| CountryError("Country database is invalid."))?;
        if !matches!(
            reader.metadata().database_type.as_str(),
            "GeoIP2-Country" | "GeoLite2-Country"
        ) {
            return Err(CountryError(
                "Country database must use the GeoIP2 or GeoLite2 country format.",
            ));
        }
        reader
            .verify()
            .map_err(|_| CountryError("Country database verification failed."))?;
        Ok(Self(reader))
    }

    pub fn lookup(&self, peer: IpAddr) -> Result<Country, CountryError> {
        let result = self
            .0
            .lookup(peer.to_canonical())
            .map_err(|_| CountryError("Country lookup is unavailable."))?;
        let decoded = result
            .decode::<maxminddb::geoip2::Country>()
            .map_err(|_| CountryError("Country lookup is unavailable."))?;
        let Some(country) = decoded else {
            return Ok(Country::unknown());
        };
        let Some(code) = country.country.iso_code else {
            return Ok(Country::unknown());
        };
        let Some(name) = country.country.names.english else {
            return Err(CountryError("Country display name is unavailable."));
        };
        if !country_code(code)
            || name.is_empty()
            || name.len() > 100
            || name.chars().any(char::is_control)
        {
            return Err(CountryError("Country display data is invalid."));
        }
        Ok(Country {
            code: code.to_owned(),
            name: name.to_owned(),
        })
    }
}

// Release-owned sprite classes; an unknown database code must not display the
// sprite's first flag by accident. This set comes from the pinned public CSS.
pub fn country_code(value: &str) -> bool {
    value.len() == 2
        && COUNTRY_CODES
            .split_ascii_whitespace()
            .any(|code| code == value)
}

const COUNTRY_CODES: &str = "AD AE AF AG AI AL AM AN AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CS CU CV CW CX CY CZ DE DJ DK DM DO DZ EC EE EG EH XE ER ES ET EU FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC XS SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU XW WF WS XK XX YE YT ZA ZM ZW";

/// The pinned public board-flag dictionary and select order.
pub const BOARD_FLAGS: &[(&str, &str)] = &[
    ("AC", "Anarcho-Capitalist"),
    ("AN", "Anarchist"),
    ("BL", "Black Nationalist"),
    ("CF", "Confederate"),
    ("CM", "Communist"),
    ("CT", "Catalonia"),
    ("DM", "Democrat"),
    ("EU", "European"),
    ("FC", "Fascist"),
    ("GN", "Gadsden"),
    ("GY", "Gay"),
    ("JH", "Jihadi"),
    ("KN", "Kekistani"),
    ("MF", "Muslim"),
    ("NB", "National Bolshevik"),
    ("NT", "NATO"),
    ("NZ", "Nazi"),
    ("PC", "Hippie"),
    ("PR", "Pirate"),
    ("RE", "Republican"),
    ("MZ", "Task Force Z"),
    ("TM", "Templar"),
    ("TR", "Tree Hugger"),
    ("UN", "United Nations"),
    ("WP", "White Supremacist"),
];

pub fn board_flag(value: &str) -> Option<&'static str> {
    BOARD_FLAGS
        .iter()
        .find(|(code, _)| *code == value)
        .map(|(_, name)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> CountryDatabase {
        CountryDatabase::from_bytes(
            include_bytes!("../tests/fixtures/GeoIP2-Country-Test.mmdb").to_vec(),
        )
        .unwrap()
    }
    #[test]
    fn producer_fixture_maps_both_address_families_and_unknowns() {
        let db = database();
        for (peer, code, name) in [
            ("81.2.69.142", "GB", "United Kingdom"),
            ("::ffff:81.2.69.142", "GB", "United Kingdom"),
            ("2001:218::", "JP", "Japan"),
            ("192.0.2.10", "XX", "Unknown"),
        ] {
            assert_eq!(
                db.lookup(peer.parse().unwrap()).unwrap(),
                Country {
                    code: code.into(),
                    name: name.into()
                }
            );
        }
    }
    #[test]
    fn invalid_sources_and_unrecognized_display_codes_are_rejected() {
        assert!(CountryDatabase::from_bytes(vec![]).is_err());
        assert!(CountryDatabase::from_bytes(vec![0; 1024]).is_err());
        assert!(CountryDatabase::load(Path::new("relative.mmdb")).is_err());
        for invalid in ["gb", "ZZ", "GB onclick=x", "", "GＢ"] {
            assert!(!country_code(invalid));
        }
        assert!(country_code("GB") && country_code("XX"));
        assert!(board_flag("AC").is_some() && board_flag("ac").is_none());
    }

    #[test]
    fn absolute_operator_file_is_loaded_and_directories_are_rejected() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let db = CountryDatabase::load(&directory.join("GeoIP2-Country-Test.mmdb")).unwrap();
        assert_eq!(
            db.lookup("81.2.69.142".parse().unwrap()).unwrap().code,
            "GB"
        );
        assert!(CountryDatabase::load(&directory).is_err());
        assert!(CountryDatabase::load(&directory.join("MAXMIND-LICENSE-APACHE.txt")).is_err());
    }
}
