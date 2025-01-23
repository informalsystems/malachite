use async_trait::async_trait;
use malachitebft_core_consensus::types::*;

#[async_trait]
pub trait SigningHandler<Ctx>
where
    Ctx: Context,
{
    /// Sign a vote with this node's private key
    async fn sign_vote(&mut self, vote: Ctx::Vote) -> SignedVote<Ctx>;

    /// Sign a proposal with this node's private key
    async fn sign_proposal(&mut self, proposal: Ctx::Proposal) -> SignedProposal<Ctx>;

    /// Verify a signature
    async fn verify_signature(
        &mut self,
        signed_msg: SignedMessage<Ctx, ConsensusMsg<Ctx>>,
        public_key: PublicKey<Ctx>,
    ) -> bool;

    /// Verify a commit certificate
    async fn verify_certificate(
        &mut self,
        certificate: CommitCertificate<Ctx>,
        validator_set: Ctx::ValidatorSet,
        threshold_params: ThresholdParams,
    ) -> Result<(), CertificateError<Ctx>>;
}
