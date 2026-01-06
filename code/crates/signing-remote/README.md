# HTTP Remote Signing Provider

HTTP client for delegating ECDSA K256 (secp256k1) signing operations to a remote service.

## Usage

```rust
use malachitebft_signing_remote::HttpRemoteSigner;
use std::time::Duration;

// Create the remote signer client
let signer = HttpRemoteSigner::new(
    "https://signer.example.com".to_string(),
    "abcdef1234567890abcdef1234567890".to_string(),  // exactly 32 chars
    Duration::from_secs(5),
)?;

// Sign message bytes
let message = b"Hello, world!";
let signature = signer.sign_bytes(message).await?;
```

## Configuration

- **endpoint**: HTTP(S) URL of your remote signer service
- **auth_token**: Authentication token (must be exactly 32 characters)
- **timeout**: Request timeout duration

Generate a secure token:
```bash
openssl rand -hex 32
```

## Remote Signer API

Your remote signing service must implement this endpoint:

### POST `/`

**Request Headers:**
```
Authorization: Bearer <32-char-token>
Content-Type: application/json
```

**Request Body:**
```json
{
  "message": "base64-encoded-message-bytes"
}
```

**Response (200 OK):**
```json
{
  "signature": "base64-encoded-ecdsa-k256-signature-bytes"
}
```

**Error Response:**
```json
{
  "error": "error description"
}
```

The signature must be a valid ECDSA K256 (secp256k1) signature in DER format, base64-encoded.

## Security

- Always use HTTPS in production
- Keep auth tokens secret (treat like private keys)
- Use cryptographically random 32-character tokens
