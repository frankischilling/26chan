use ring::{
    digest::{Context, SHA256},
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use std::net::IpAddr;

/// A deletion capability. Deliberately has no Debug or serialization derive.
/// Only the cookie encoder and password verifier may use its cleartext form.
#[derive(Clone)]
pub struct Capability([u8; 32]);

/// Server-owned, session-specific hashes; these never enter public projections.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Fingerprints {
    pub token: [u8; 32],
    pub network: [u8; 32],
    pub address: [u8; 32],
    pub environment: [u8; 32],
}

impl Capability {
    pub fn generate() -> Result<Self, ring::error::Unspecified> {
        let mut bytes = [0; 32];
        SystemRandom::new().fill(&mut bytes)?;
        Ok(Self(bytes))
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.strip_prefix("a1_")?;
        if value.len() != 64 {
            return None;
        }
        let mut bytes = [0; 32];
        for (output, input) in bytes.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
            *output = nibble(input[0])? * 16 + nibble(input[1])?;
        }
        Some(Self(bytes))
    }

    pub fn credential(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut value = String::with_capacity(67);
        value.push_str("a1_");
        for byte in self.0 {
            value.push(char::from(HEX[usize::from(byte >> 4)]));
            value.push(char::from(HEX[usize::from(byte & 15)]));
        }
        value
    }

    pub fn storage_hash(&self) -> [u8; 32] {
        let mut hash = Context::new(&SHA256);
        hash.update(b"26chan:anonymous-capability:v1\0");
        hash.update(&self.0);
        hash.finish().as_ref().try_into().expect("SHA-256 size")
    }

    /// Country comes from the trusted GeoIP result, never a request header.
    /// A missing transport identity has a distinct development-only sentinel.
    pub fn fingerprints(&self, peer: Option<IpAddr>, country: [u8; 2]) -> Fingerprints {
        let peer = peer.map(|peer| match peer {
            IpAddr::V6(address) => address.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(peer),
            _ => peer,
        });
        let mut address_bytes = [0; 17];
        let address_length;
        let network_length;
        match peer {
            Some(IpAddr::V4(address)) => {
                address_bytes[0] = 4;
                address_bytes[1..5].copy_from_slice(&address.octets());
                address_length = 5;
                // get_ip_mask retains the first two IPv4 octets.
                network_length = 3;
            }
            Some(IpAddr::V6(address)) => {
                address_bytes[0] = 6;
                address_bytes[1..17].copy_from_slice(&address.octets());
                address_length = 17;
                network_length = 9;
            }
            None => {
                address_length = 1;
                network_length = 1;
            }
        }
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.0);
        let fingerprint = |purpose: &[u8], data: &[u8]| {
            let mut hash = hmac::Context::with_key(&key);
            hash.update(b"26chan:anonymous-context:v1\0");
            hash.update(purpose);
            hash.update(b"\0");
            hash.update(data);
            hash.sign().as_ref().try_into().expect("HMAC-SHA256 size")
        };
        Fingerprints {
            token: self.storage_hash(),
            network: fingerprint(b"network", &address_bytes[..network_length]),
            address: fingerprint(b"address", &address_bytes[..address_length]),
            environment: fingerprint(b"country", &country),
        }
    }
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}
