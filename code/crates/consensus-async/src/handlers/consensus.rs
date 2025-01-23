use async_trait::async_trait;
use malachitebft_core_consensus::types::*;

#[async_trait]
pub trait ConsensusHandler<Ctx>
where
    Ctx: Context,
{
    type Error: core::error::Error;

    /// Consensus is starting a new round with the given proposer
    async fn start_round(
        &mut self,
        height: Ctx::Height,
        round: Round,
        proposer: Ctx::Address,
    ) -> Result<(), Self::Error>;

    /// Publish a message to peers
    async fn publish(&mut self, msg: SignedConsensusMsg<Ctx>) -> Result<(), Self::Error>;

    /// Requests the application to build a value for consensus to run on.
    async fn get_value(
        &mut self,
        height: Ctx::Height,
        round: Round,
        timeout: Timeout,
    ) -> Result<(), Self::Error>;

    /// Get the validator set at the given height
    async fn get_validator_set(
        &mut self,
        height: Ctx::Height,
    ) -> Result<Option<Ctx::ValidatorSet>, Self::Error>;

    /// Requests the application to re-stream a proposal that it has already seen.
    async fn restream_value(
        &mut self,
        height: Ctx::Height,
        round: Round,
        valid_round: Round,
        proposer: Ctx::Address,
        value_id: ValueId<Ctx>,
    ) -> Result<(), Self::Error>;

    /// Notifies the application that consensus has decided on a value.
    async fn decide(&mut self, certificate: CommitCertificate<Ctx>) -> Result<(), Self::Error>;
}
