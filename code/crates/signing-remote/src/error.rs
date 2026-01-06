#[derive(Debug, thiserror::Error)]
pub enum RemoteSignerError {
    #[error("Authentication token must be exactly {required} characters, got {actual} characters")]
    InvalidAuthToken { required: usize, actual: usize },

    #[error("Failed to create HTTP client: {0}")]
    ClientCreation(String),

    #[error("Remote signer request failed: {0}")]
    Request(String),

    #[error("Remote signer returned HTTP {status}: {body}")]
    HttpError { status: u16, body: String },

    #[error("Failed to parse remote signer response: {0}")]
    ResponseParse(String),

    #[error("Failed to decode base64: {0}")]
    Base64Decode(String),

    #[error("Failed to parse signature: {0}")]
    SignatureParse(String),
}
