use address::Address;
use error::ClarityError;
use failure::Error;
use num256::Uint256;
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use sha3::{Digest, Keccak256};
use signature::Signature;
use std::str::FromStr;
use utils::{bytes_to_hex_str, hex_str_to_bytes, ByteDecodeError};

#[derive(Fail, Debug, PartialEq)]
pub enum PrivateKeyError {
    #[fail(display = "Private key should be exactly 64 bytes")]
    InvalidLengthError,
}

/// Representation of an Ethereum private key.
///
/// Private key can be created using a textual representation,
/// a raw binary form using array of bytes.
///
/// With PrivateKey you are able to sign messages, derive
/// public keys. Cryptography-related methods use
/// SECP256K1 elliptic curves.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub struct PrivateKey([u8; 32]);

impl FromStr for PrivateKey {
    type Err = Error;

    /// Parse a textual representation of a private key back into PrivateKey type.
    ///
    /// It has to be a string that represents 64 characters that are hexadecimal
    /// representation of 32 bytes. Optionally this string can be prefixed with `0x`
    /// at the beggining.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(PrivateKeyError::InvalidLengthError.into());
        }
        let bytes = hex_str_to_bytes(&s)?;
        debug_assert_eq!(bytes.len(), 32);
        let mut res = [0x0u8; 32];
        res.copy_from_slice(&bytes[..]);
        Ok(PrivateKey(res))
    }
}

impl From<[u8; 32]> for PrivateKey {
    fn from(val: [u8; 32]) -> PrivateKey {
        PrivateKey(val)
    }
}

impl PrivateKey {
    /// Creates a null private key that uses zeros.
    pub fn new() -> PrivateKey {
        PrivateKey([0u8; 32])
    }

    /// Convert a given slice of bytes into a valid private key.
    ///
    /// Input bytes are validated for a length only.
    ///
    /// * `slice` - A slice of raw bytes with a length of 32.
    pub fn from_slice(slice: &[u8]) -> Result<PrivateKey, Error> {
        if slice.len() != 32 {
            return Err(ClarityError::InvalidPrivKey.into());
        }
        let mut res = [0u8; 32];
        res.copy_from_slice(slice);
        Ok(PrivateKey(res))
    }

    /// Get bytes back from a PrivateKey
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    /// Create a public key for a given private key.
    ///
    /// This is well explained in the Ethereum Yellow Paper Appendix F.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use clarity::PrivateKey;
    /// let private_key : PrivateKey = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f1e".parse().unwrap();
    /// let public_key = private_key.to_public_key().unwrap();
    /// ```
    pub fn to_public_key(&self) -> Result<Address, Error> {
        let secp256k1 = Secp256k1::new();
        let sk = SecretKey::from_slice(&secp256k1, &self.0)?;
        let pkey = PublicKey::from_secret_key(&secp256k1, &sk);
        // TODO: This part is duplicated with sender code.

        // Serialize the recovered public key in uncompressed format
        let pkey = pkey.serialize_uncompressed();
        assert_eq!(pkey.len(), 65);
        if pkey[1..].to_vec() == [0x00u8; 64].to_vec() {
            return Err(ClarityError::ZeroPrivKey.into());
        }
        // Finally an address is last 20 bytes of a hash of the public key.
        let sender = Keccak256::digest(&pkey[1..]);
        debug_assert_eq!(sender.len(), 32);
        Address::from_slice(&sender[12..])
    }
    /// Signs any message that is represented by a data buffer
    pub fn sign_hash(&self, data: &[u8]) -> Signature {
        debug_assert_eq!(data.len(), 32);
        // Sign RLP encoded data
        let full = Secp256k1::new(); // TODO: in original libsecp256k1 source code there is a suggestion that the context should be kept for the duration of the program.
                                     // TODO: secp256k1 types could be hidden somehow
        let msg = Message::from_slice(&data).unwrap();
        let sk = SecretKey::from_slice(&full, &self.to_bytes()).unwrap();
        // Sign the raw hash of RLP encoded transaction data with a private key.
        let sig = full.sign_recoverable(&msg, &sk);
        // Serialize the signature into the "compact" form which means
        // it will be exactly 64 bytes, and the "excess" information of
        // recovery id will be given to us.
        let (recovery_id, compact) = sig.serialize_compact(&full);
        debug_assert_eq!(compact.len(), 64);
        // I assume recovery ID is always greater than 0 to simplify
        // the conversion from i32 to Uint256. On a side note,
        // I believe "v" could be an u64 value (TODO).
        let recovery_id = recovery_id.to_i32();
        assert!(recovery_id >= 0);
        let recovery_id = recovery_id as u32;
        let v: Uint256 = (recovery_id + 27).into();
        let r = Uint256::from_bytes_be(&compact[0..32]);
        let s = Uint256::from_bytes_be(&compact[32..64]);
        // This will swap the signature of a transaction, and returns a new signed TX.
        Signature::new(v, r, s)
    }
    /// Signs a message. This makes a hash of data, and then
    /// makes prepares a signature of it.
    pub fn sign_msg(&self, data: &[u8]) -> Signature {
        let digest = Keccak256::digest(data);
        self.sign_hash(&digest)
    }
}

impl ToString for PrivateKey {
    /// Converts PrivateKey into a textual representation.
    fn to_string(&self) -> String {
        format!("0x{}", bytes_to_hex_str(&self.to_bytes()))
    }
}

impl Serialize for PrivateKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PrivateKey {
    fn deserialize<D>(deserializer: D) -> Result<PrivateKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = if s.starts_with("0x") { &s[2..] } else { &s };

        hex_str_to_bytes(&s)
            .and_then(move |bytes| PrivateKey::from_slice(&bytes))
            .map_err(serde::de::Error::custom)

        // Ok(PrivateKey::from_slice(&bytes)?)
        //
    }
}

#[test]
#[should_panic]
fn too_short() {
    PrivateKey::from_str("abcdef").unwrap();
}

#[test]
#[should_panic]
fn invalid_data() {
    let key = "\u{012345}c85ef7d79691fe79573b1a7064c19c1a9819ebdbd1faaab1a8ec92344438";
    assert_eq!(key.len(), 64);
    PrivateKey::from_str(key).unwrap();
}

#[test]
fn parse_address_1() {
    use utils::bytes_to_hex_str;
    // https://github.com/ethereum/tests/blob/b44cea1cccf1e4b63a05d1ca9f70f2063f28da6d/BasicTests/txtest.json
    let key: PrivateKey = "c85ef7d79691fe79573b1a7064c19c1a9819ebdbd1faaab1a8ec92344438aaf4"
        .parse()
        .unwrap();
    assert_eq!(
        key.to_bytes(),
        [
            0xc8, 0x5e, 0xf7, 0xd7, 0x96, 0x91, 0xfe, 0x79, 0x57, 0x3b, 0x1a, 0x70, 0x64, 0xc1,
            0x9c, 0x1a, 0x98, 0x19, 0xeb, 0xdb, 0xd1, 0xfa, 0xaa, 0xb1, 0xa8, 0xec, 0x92, 0x34,
            0x44, 0x38, 0xaa, 0xf4
        ]
    );

    // geth account import <(echo c85ef7d79691fe79573b1a7064c19c1a9819ebdbd1faaab1a8ec92344438aaf4)
    assert_eq!(
        bytes_to_hex_str(&key.to_public_key().unwrap().as_bytes()),
        "cd2a3d9f938e13cd947ec05abc7fe734df8dd826"
    );
}

#[test]
fn parse_address_2() {
    use utils::bytes_to_hex_str;
    // https://github.com/ethereum/tests/blob/b44cea1cccf1e4b63a05d1ca9f70f2063f28da6d/BasicTests/txtest.json
    let key: PrivateKey = "c87f65ff3f271bf5dc8643484f66b200109caffe4bf98c4cb393dc35740b28c0"
        .parse()
        .unwrap();
    assert_eq!(
        key.to_bytes(),
        [
            0xc8, 0x7f, 0x65, 0xff, 0x3f, 0x27, 0x1b, 0xf5, 0xdc, 0x86, 0x43, 0x48, 0x4f, 0x66,
            0xb2, 0x00, 0x10, 0x9c, 0xaf, 0xfe, 0x4b, 0xf9, 0x8c, 0x4c, 0xb3, 0x93, 0xdc, 0x35,
            0x74, 0x0b, 0x28, 0xc0
        ]
    );

    // geth account import <(echo c87f65ff3f271bf5dc8643484f66b200109caffe4bf98c4cb393dc35740b28c0)
    assert_eq!(
        bytes_to_hex_str(&key.to_public_key().unwrap().as_bytes()),
        "13978aee95f38490e9769c39b2773ed763d9cd5f"
    );
}

#[test]
#[should_panic]
fn zero_address() {
    // A key full of zeros is an invalid private key.
    let key = PrivateKey::new();
    key.to_public_key().unwrap();
}

#[test]
fn sign_message() {
    use utils::bytes_to_hex_str;
    // https://github.com/ethereum/tests/blob/b44cea1cccf1e4b63a05d1ca9f70f2063f28da6d/BasicTests/txtest.json
    let key: PrivateKey = "c87f65ff3f271bf5dc8643484f66b200109caffe4bf98c4cb393dc35740b28c0"
        .parse()
        .unwrap();
    assert_eq!(
        key.to_bytes(),
        [
            0xc8, 0x7f, 0x65, 0xff, 0x3f, 0x27, 0x1b, 0xf5, 0xdc, 0x86, 0x43, 0x48, 0x4f, 0x66,
            0xb2, 0x00, 0x10, 0x9c, 0xaf, 0xfe, 0x4b, 0xf9, 0x8c, 0x4c, 0xb3, 0x93, 0xdc, 0x35,
            0x74, 0x0b, 0x28, 0xc0
        ]
    );

    let hash = Keccak256::digest(&"Hello, world!".as_bytes());

    // geth account import <(echo c87f65ff3f271bf5dc8643484f66b200109caffe4bf98c4cb393dc35740b28c0)
    let sig = key.sign_hash(&hash);
    assert_eq!(sig.v, 27u32.into());
    assert_eq!(
        sig.r,
        "60846573560682549108588594828362990367411621835316234394067988873897934296519"
            .parse()
            .unwrap()
    );
    assert_eq!(
        sig.s,
        "38796436849307511461301231459196686786518980571289303247679628937607287361713"
            .parse()
            .unwrap()
    );

    let sig_2 = key.sign_msg(&"Hello, world!".as_bytes());
    assert_eq!(sig, sig_2);
}

#[test]
fn serialize_to_json() {
    let unsafe_key: PrivateKey = "0101010101010101010101010101010101010101010101010101010101010101"
        .parse()
        .unwrap();
    let j = serde_json::to_string(&unsafe_key).unwrap();
    assert_eq!(
        j,
        r#""0x0101010101010101010101010101010101010101010101010101010101010101""#
    );
    let recovered_key: PrivateKey = serde_json::from_str(&j).unwrap();
    assert_eq!(unsafe_key, recovered_key);
}
