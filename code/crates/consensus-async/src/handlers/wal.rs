use async_trait::async_trait;
use malachitebft_core_consensus::types::*;

#[async_trait]
pub trait WalHandler<Ctx>
where
    Ctx: Context,
{
    type Error: core::error::Error;

    /// Append a consensus message to the Write-Ahead Log for crash recovery
    async fn append_msg(&mut self, msg: SignedConsensusMsg<Ctx>) -> Result<(), Self::Error>;

    /// Append a timeout to the Write-Ahead Log for crash recovery
    async fn append_timeout(&mut self, timeout: Timeout) -> Result<(), Self::Error>;
}
