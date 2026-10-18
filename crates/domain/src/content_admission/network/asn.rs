//! Offline ASN lookup using an operator-supplied database. No request header
//! can select the database, address or an asserted autonomous-system number.
use std::{io::Read, net::IpAddr, path::Path};

const MAX_DATABASE_BYTES: u64 = 64 * 1024 * 1024;

pub struct Database(maxminddb::Reader<Vec<u8>>);

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub &'static str);

impl Database {
    pub fn load(path: &Path) -> Result<Self, Error> {
        if !path.is_absolute() {
            return Err(Error("ASN_DATABASE must be an absolute file path."));
        }
        let before =
            std::fs::metadata(path).map_err(|_| Error("ASN database could not be inspected."))?;
        if !before.is_file() || before.len() == 0 || before.len() > MAX_DATABASE_BYTES {
            return Err(Error(
                "ASN database must be a regular file of at most 64 MiB.",
            ));
        }
        let file =
            std::fs::File::open(path).map_err(|_| Error("ASN database could not be opened."))?;
        let metadata = file
            .metadata()
            .map_err(|_| Error("ASN database could not be inspected."))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_DATABASE_BYTES {
            return Err(Error(
                "ASN database must be a regular file of at most 64 MiB.",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_DATABASE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error("ASN database could not be read."))?;
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, Error> {
        if bytes.is_empty() || bytes.len() as u64 > MAX_DATABASE_BYTES {
            return Err(Error("ASN database size is invalid."));
        }
        let reader =
            maxminddb::Reader::from_source(bytes).map_err(|_| Error("ASN database is invalid."))?;
        if reader.metadata().database_type != "GeoLite2-ASN" {
            return Err(Error("ASN database must use the GeoLite2-ASN format."));
        }
        reader
            .verify()
            .map_err(|_| Error("ASN database verification failed."))?;
        Ok(Self(reader))
    }

    /// Unknown addresses have no ASN. Decode errors remain errors, never an
    /// invented zero or a caller-provided fallback. IPv4-mapped IPv6 is canonical.
    pub fn lookup(&self, peer: IpAddr) -> Result<Option<u32>, Error> {
        let result = self
            .0
            .lookup(peer.to_canonical())
            .map_err(|_| Error("ASN lookup is unavailable."))?;
        let decoded = result
            .decode::<maxminddb::geoip2::Asn>()
            .map_err(|_| Error("ASN lookup is unavailable."))?;
        match decoded.and_then(|value| value.autonomous_system_number) {
            Some(0) => Err(Error("ASN lookup returned an invalid number.")),
            value => Ok(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> Database {
        Database::from_bytes(
            include_bytes!("../../../tests/fixtures/GeoLite2-ASN-Test.mmdb").to_vec(),
        )
        .unwrap()
    }
    #[test]
    fn producer_fixture_resolves_canonical_addresses_and_unknowns() {
        let db = database();
        for (peer, expected) in [
            ("1.0.0.1", Some(15169)),
            ("::ffff:1.0.0.1", Some(15169)),
            ("1.128.0.1", Some(1221)),
            ("2001:1700::1", Some(6730)),
            ("192.0.2.10", None),
        ] {
            assert_eq!(
                db.lookup(peer.parse().unwrap()).unwrap(),
                expected,
                "{peer}"
            );
        }
    }
    #[test]
    fn unrelated_invalid_and_relative_databases_are_rejected() {
        assert!(Database::from_bytes(Vec::new()).is_err());
        assert!(Database::from_bytes(vec![0; 1024]).is_err());
        assert!(
            Database::from_bytes(
                include_bytes!("../../../tests/fixtures/GeoIP2-Country-Test.mmdb").to_vec()
            )
            .is_err()
        );
        assert!(Database::load(Path::new("relative.mmdb")).is_err());
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        assert!(Database::load(&directory).is_err());
        let db = Database::load(&directory.join("GeoLite2-ASN-Test.mmdb")).unwrap();
        assert_eq!(db.lookup("1.0.0.1".parse().unwrap()).unwrap(), Some(15169));
    }
}
