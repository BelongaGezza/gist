//! Secret Service session crypto: `dh-ietf1024-sha256-aes128-cbc-pkcs7`
//! (freedesktop Secret Service API, "Transfer of secrets").
//!
//! Anonymous DH over the 1024-bit Second Oakley Group (RFC 2409, generator 2)
//! gives a shared value; HKDF-SHA256 (no salt, empty info) turns it into a
//! 16-byte AES key; secrets travel as AES-128-CBC with PKCS#7 padding and a
//! fresh random IV. This protects the secret on the session bus from other
//! processes, which is what libsecret does. The group is small by modern
//! standards, and `num-bigint`'s `modpow` is not constant time: both are
//! accepted because the exchange is ephemeral, local and per connection, and
//! it is what every Secret Service speaks. The `plain` algorithm is never used.

use aes::Aes128;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use hkdf::Hkdf;
use num_bigint::BigUint;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::KeyError;

pub const ALGORITHM: &str = "dh-ietf1024-sha256-aes128-cbc-pkcs7";
const GROUP_BYTES: usize = 128;
const IV_LEN: usize = 16;

/// RFC 2409 section 6.2, "Second Oakley Group" prime.
const PRIME_HEX: &str = "\
FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74\
020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F1437\
4FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED\
EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE65381FFFFFFFFFFFFFFFF";

fn prime() -> BigUint {
    BigUint::parse_bytes(PRIME_HEX.as_bytes(), 16).expect("valid prime constant")
}

fn fixed_be(value: &BigUint) -> Vec<u8> {
    let bytes = value.to_bytes_be();
    let mut out = vec![0u8; GROUP_BYTES.saturating_sub(bytes.len())];
    out.extend_from_slice(&bytes);
    out
}

fn crypto(message: &str) -> KeyError {
    KeyError::Store(format!("session crypto: {message}"))
}

/// One side of the exchange.
pub struct KeyPair {
    private: Zeroizing<Vec<u8>>,
    public: Vec<u8>,
}

impl KeyPair {
    pub fn generate() -> Result<Self, KeyError> {
        let mut private = Zeroizing::new(vec![0u8; GROUP_BYTES]);
        getrandom::fill(private.as_mut_slice()).map_err(|e| crypto(&e.to_string()))?;
        let public = prime_pow(&BigUint::from(2u32), &private);
        Ok(Self {
            private,
            public: fixed_be(&public),
        })
    }

    #[cfg(test)]
    fn from_private(private: Vec<u8>) -> Self {
        let public = fixed_be(&prime_pow(&BigUint::from(2u32), &private));
        Self {
            private: Zeroizing::new(private),
            public,
        }
    }

    pub fn public_bytes(&self) -> &[u8] {
        &self.public
    }

    /// Combine with the peer's public value. Rejects degenerate values
    /// (0, 1, p-1 and anything >= p) that would force a predictable secret.
    pub fn derive(&self, peer_public: &[u8]) -> Result<SessionKey, KeyError> {
        let p = prime();
        let peer = BigUint::from_bytes_be(peer_public);
        let one = BigUint::from(1u32);
        if peer <= one || peer >= &p - &one {
            return Err(crypto("peer public value is out of range"));
        }
        let shared = Zeroizing::new(fixed_be(&prime_pow(&peer, &self.private)));
        let mut key = Zeroizing::new([0u8; 16]);
        Hkdf::<Sha256>::new(None, shared.as_slice())
            .expand(&[], key.as_mut_slice())
            .map_err(|_| crypto("key derivation failed"))?;
        Ok(SessionKey { key })
    }
}

fn prime_pow(base: &BigUint, exponent_be: &[u8]) -> BigUint {
    base.modpow(&BigUint::from_bytes_be(exponent_be), &prime())
}

/// Negotiated AES-128 session key.
pub struct SessionKey {
    key: Zeroizing<[u8; 16]>,
}

impl SessionKey {
    /// Returns `(iv, ciphertext)`.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), KeyError> {
        let mut iv = [0u8; IV_LEN];
        getrandom::fill(&mut iv).map_err(|e| crypto(&e.to_string()))?;
        let cipher = cbc::Encryptor::<Aes128>::new_from_slices(self.key.as_slice(), &iv)
            .map_err(|_| crypto("bad key or iv length"))?;
        Ok((iv.to_vec(), cipher.encrypt_padded_vec::<Pkcs7>(plaintext)))
    }

    pub fn decrypt(&self, iv: &[u8], ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>, KeyError> {
        let cipher = cbc::Decryptor::<Aes128>::new_from_slices(self.key.as_slice(), iv)
            .map_err(|_| crypto("bad iv length"))?;
        cipher
            .decrypt_padded_vec::<Pkcs7>(ciphertext)
            .map(Zeroizing::new)
            .map_err(|_| crypto("bad padding or ciphertext"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prime_is_the_1024_bit_second_oakley_group() {
        let p = prime();
        assert_eq!(p.bits(), 1024);
        assert_eq!(p.to_bytes_be().len(), GROUP_BYTES);
        // Safe-prime shape used by this group: p = 2^1024 - 2^960 - 1 + 2^64 * (...), odd.
        assert_eq!(&p % 2u32, BigUint::from(1u32));
        assert_eq!(&p % 4u32, BigUint::from(3u32));
    }

    /// Known answer computed independently in Python (pow() for the group,
    /// hmac/hashlib for HKDF-SHA256 with zero salt and empty info): pins the
    /// byte order, padding and KDF parameters so a self-consistent mistake
    /// cannot slip through the client/fake-server tests.
    #[test]
    fn derives_the_independently_computed_key() {
        let a: Vec<u8> = (0..128).map(|i| ((i * 3 + 1) % 256) as u8).collect();
        let b: Vec<u8> = (0..128).map(|i| ((i * 7 + 5) % 256) as u8).collect();
        let (client, server) = (KeyPair::from_private(a), KeyPair::from_private(b));
        assert_eq!(
            &server.public_bytes()[..8],
            &[0x11, 0xf1, 0x9e, 0x72, 0xdd, 0x1d, 0xb3, 0xd2]
        );
        let expected = [
            0x4c, 0x74, 0x67, 0x86, 0x6e, 0x4c, 0x38, 0x5e, 0x1f, 0x74, 0xa9, 0xd6, 0x25, 0x1b,
            0x54, 0x11,
        ];
        for key in [
            client.derive(server.public_bytes()).unwrap(),
            server.derive(client.public_bytes()).unwrap(),
        ] {
            assert_eq!(key.key.as_slice(), expected.as_slice());
        }
    }

    #[test]
    fn both_sides_derive_the_same_key_and_round_trip_a_secret() {
        let (client, server) = (KeyPair::generate().unwrap(), KeyPair::generate().unwrap());
        let a = client.derive(server.public_bytes()).unwrap();
        let b = server.derive(client.public_bytes()).unwrap();
        let secret = [0xA5u8; 32];
        let (iv, ct) = a.encrypt(&secret).unwrap();
        assert_ne!(ct.as_slice(), secret.as_slice());
        assert_eq!(ct.len(), 48, "32 bytes + a full PKCS#7 padding block");
        assert_eq!(b.decrypt(&iv, &ct).unwrap().as_slice(), secret.as_slice());
    }

    #[test]
    fn a_different_peer_cannot_decrypt() {
        let (client, server, eve) = (
            KeyPair::generate().unwrap(),
            KeyPair::generate().unwrap(),
            KeyPair::generate().unwrap(),
        );
        let (iv, ct) = client
            .derive(server.public_bytes())
            .unwrap()
            .encrypt(&[7u8; 32])
            .unwrap();
        let wrong = eve.derive(client.public_bytes()).unwrap();
        assert!(wrong
            .decrypt(&iv, &ct)
            .map(|p| p.as_slice() != [7u8; 32].as_slice())
            .unwrap_or(true));
    }

    #[test]
    fn each_encryption_uses_a_fresh_iv() {
        let (c, s) = (KeyPair::generate().unwrap(), KeyPair::generate().unwrap());
        let key = c.derive(s.public_bytes()).unwrap();
        let (iv1, ct1) = key.encrypt(&[1u8; 32]).unwrap();
        let (iv2, ct2) = key.encrypt(&[1u8; 32]).unwrap();
        assert_ne!(iv1, iv2);
        assert_ne!(ct1, ct2);
    }

    #[test]
    fn degenerate_peer_values_are_rejected() {
        let kp = KeyPair::generate().unwrap();
        let p = prime();
        let one = BigUint::from(1u32);
        for bad in [
            vec![],
            vec![0u8],
            vec![1u8],
            fixed_be(&(&p - &one)),
            fixed_be(&p),
        ] {
            assert!(kp.derive(&bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn malformed_ciphertext_and_iv_are_errors_not_panics() {
        let (c, s) = (KeyPair::generate().unwrap(), KeyPair::generate().unwrap());
        let key = c.derive(s.public_bytes()).unwrap();
        assert!(key.decrypt(&[0u8; 15], &[0u8; 16]).is_err());
        assert!(key.decrypt(&[0u8; 16], &[0u8; 15]).is_err());
        assert!(key.decrypt(&[0u8; 16], &[]).is_err());
    }
}
