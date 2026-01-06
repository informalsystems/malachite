//! HTTP Remote Signing Provider for Malachite BFT
//!
//! This crate provides HTTP client functionality for delegating signing operations
//! to a remote HTTP service, such as an HSM, KMS, or dedicated signing server.
//!
//! This library uses **ECDSA with the secp256k1 curve (K256)** for signatures.
//!
//! # Features
//!
//! - HTTP/HTTPS communication with remote signers
//! - Bearer token authentication (exactly 32 characters required)
//! - Configurable request timeouts
//! - ECDSA K256 (secp256k1) signatures

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use std::time::Duration;

use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub use malachitebft_signing_ecdsa::{Ecdsa, K256Config, PrivateKey, PublicKey, Signature};

mod error;
pub use error::RemoteSignerError;

pub const AUTH_TOKEN_LENGTH: usize = 32;

/// Request payload sent to the remote signer
#[derive(Debug, Serialize)]
struct SignRequest {
    /// Message bytes to sign, base64-encoded
    message: String,
}

/// Response payload from the remote signer
#[derive(Debug, Deserialize)]
struct SignResponse {
    /// Signature bytes, base64-encoded
    signature: String,
}

#[derive(Debug, Clone)]
pub struct HttpRemoteSigner {
    /// HTTP endpoint of the remote signer
    endpoint: String,
    /// Authentication token for the remote signer
    auth_token: String,
    /// HTTP client with connection pooling
    client: Client,
}

impl HttpRemoteSigner {
    pub fn new(
        endpoint: String,
        auth_token: String,
        timeout: Duration,
    ) -> Result<Self, RemoteSignerError> {
        if auth_token.len() != AUTH_TOKEN_LENGTH {
            return Err(RemoteSignerError::InvalidAuthToken {
                required: AUTH_TOKEN_LENGTH,
                actual: auth_token.len(),
            });
        }

        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| RemoteSignerError::ClientCreation(e.to_string()))?;

        Ok(Self {
            endpoint,
            auth_token,
            client,
        })
    }

    pub async fn sign_bytes(
        &self,
        message: &[u8],
    ) -> Result<Signature<K256Config>, RemoteSignerError> {
        let encoded_message = base64::engine::general_purpose::STANDARD.encode(message);

        let request = SignRequest {
            message: encoded_message,
        };

        let response = self
            .client
            .post(format!("{}", self.endpoint))
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| RemoteSignerError::Request(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error body".to_string());

            return Err(RemoteSignerError::HttpError {
                status: status.as_u16(),
                body: error_body,
            });
        }

        let sign_response: SignResponse = response
            .json()
            .await
            .map_err(|e| RemoteSignerError::ResponseParse(e.to_string()))?;

        let signature_bytes = base64::engine::general_purpose::STANDARD
            .decode(&sign_response.signature)
            .map_err(|e| RemoteSignerError::Base64Decode(e.to_string()))?;

        Signature::<K256Config>::from_slice(&signature_bytes)
            .map_err(|e| RemoteSignerError::SignatureParse(e.to_string()))
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_token_exactly_32_chars() {
        // Valid: exactly 32 characters
        let result = HttpRemoteSigner::new(
            "http://localhost:8080".to_string(),
            "12345678901234567890123456789012".to_string(), // exactly 32
            Duration::from_secs(5),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_auth_token_too_short() {
        // Invalid: less than 32 characters
        let result = HttpRemoteSigner::new(
            "http://localhost:8080".to_string(),
            "tooshort".to_string(), // only 8 chars
            Duration::from_secs(5),
        );
        assert!(result.is_err());
        if let Err(RemoteSignerError::InvalidAuthToken { required, actual }) = result {
            assert_eq!(required, 32);
            assert_eq!(actual, 8);
        } else {
            panic!("Expected InvalidAuthToken error");
        }
    }

    #[test]
    fn test_auth_token_too_long() {
        // Invalid: more than 32 characters
        let result = HttpRemoteSigner::new(
            "http://localhost:8080".to_string(),
            "123456789012345678901234567890123".to_string(), // 33 chars
            Duration::from_secs(5),
        );
        assert!(result.is_err());
        if let Err(RemoteSignerError::InvalidAuthToken { required, actual }) = result {
            assert_eq!(required, 32);
            assert_eq!(actual, 33);
        } else {
            panic!("Expected InvalidAuthToken error");
        }
    }
}
