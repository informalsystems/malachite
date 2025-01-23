use async_trait::async_trait;
use malachitebft_core_consensus::types::*;

#[async_trait]
pub trait SyncHandler<Ctx>
where
    Ctx: Context,
{
    type Error: core::error::Error;

    /// Consensus has been stuck in Prevote or Precommit step, ask for vote sets from peers
    async fn get_vote_set(&mut self, height: Ctx::Height, round: Round) -> Result<(), Self::Error>;

    /// A peer has required our vote set, send the response
    async fn send_vote_set_response(
        &mut self,
        request_id: RequestId,
        height: Ctx::Height,
        round: Round,
        vote_set: VoteSet<Ctx>,
    );
}
