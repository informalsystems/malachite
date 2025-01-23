use async_trait::async_trait;
use malachitebft_core_consensus::types::*;

#[async_trait]
pub trait TimeoutHandler {
    type Error: core::error::Error;

    /// Reset all timeouts to their initial values
    async fn reset_timeouts(&mut self) -> Result<(), Self::Error>;

    /// Cancel all outstanding timeouts
    async fn cancel_all_timeouts(&mut self) -> Result<(), Self::Error>;

    /// Cancel a given timeout
    async fn cancel_timeout(&mut self, timeout: Timeout) -> Result<(), Self::Error>;

    /// Schedule a timeout
    async fn schedule_timeout(&mut self, timeout: Timeout) -> Result<(), Self::Error>;
}
