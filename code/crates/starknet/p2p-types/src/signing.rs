use core::fmt;

use malachite_common::SigningScheme;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use starknet_core::crypto::{ecdsa_sign, ecdsa_verify};
use starknet_crypto::{get_public_key, poseidon_hash_single, Felt};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Ecdsa;

impl Ecdsa {
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn generate_keypair<R>(rng: R) -> PrivateKey
    where
        R: RngCore + CryptoRng,
    {
        PrivateKey::generate(rng)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct InvalidSignatureLength(usize);

impl fmt::Display for InvalidSignatureLength {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Invalid signature length: got {}, expected {}",
            self.0,
            32 * 3
        )
    }
}

impl SigningScheme for Ecdsa {
    type DecodingError = InvalidSignatureLength;

    type Signature = Signature;
    type PublicKey = PublicKey;
    type PrivateKey = PrivateKey;

    fn encode_signature(signature: &Self::Signature) -> Vec<u8> {
        let mut result = Vec::with_capacity(64);
        result.extend_from_slice(&signature.0.r.to_bytes_be());
        result.extend_from_slice(&signature.0.s.to_bytes_be());
        result
    }

    fn decode_signature(bytes: &[u8]) -> Result<Self::Signature, Self::DecodingError> {
        if bytes.len() != 32 * 2 {
            return Err(InvalidSignatureLength(bytes.len()));
        }

        let r = Felt::from_bytes_be_slice(&bytes[0..32]);
        let s = Felt::from_bytes_be_slice(&bytes[32..64]);

        Ok(Signature(starknet_crypto::Signature { r, s }))
    }
}

#[derive(Debug)]
pub struct Signature(starknet_crypto::Signature);

impl Signature {
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn inner(&self) -> &starknet_crypto::Signature {
        &self.0
    }
}

impl Clone for Signature {
    fn clone(&self) -> Self {
        Self(starknet_crypto::Signature {
            r: self.0.r,
            s: self.0.s,
        })
    }
}

impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.0.r == other.0.r && self.0.s == other.0.s
    }
}

impl Eq for Signature {}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrivateKey(Felt);

impl PrivateKey {
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn generate<R>(mut rng: R) -> Self
    where
        R: RngCore + CryptoRng,
    {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        Self::from(bytes)
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn public_key(&self) -> PublicKey {
        PublicKey::new(get_public_key(&self.0))
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn inner(&self) -> Felt {
        self.0
    }

    pub fn sign(&self, message: &Felt) -> Signature {
        let signature = ecdsa_sign(&self.0, message).unwrap();
        Signature(signature.into())
    }
}

impl From<[u8; 32]> for PrivateKey {
    fn from(bytes: [u8; 32]) -> Self {
        Self(Felt::from_bytes_be(&bytes))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PublicKey(Felt);

impl PublicKey {
    pub fn new(key: Felt) -> Self {
        Self(key)
    }

    pub fn hash(&self) -> Felt {
        poseidon_hash_single(self.0)
    }

    pub fn inner(&self) -> Felt {
        self.0
    }

    pub fn verify(&self, message: &Felt, signature: &Signature) -> bool {
        ecdsa_verify(&self.0, message, &signature.0).unwrap()
    }
}
