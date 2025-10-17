use async_trait::async_trait;
use bytes::Bytes;

use malachitebft_core_types::{SignedExtension, SignedProposal, SignedProposalPart, SignedVote};
use malachitebft_signing::{Error, SigningProvider, VerificationResult};

use crate::{Proposal, ProposalPart, TestContext, Vote};

pub use malachitebft_signing_ed25519::*;

pub trait Hashable {
    type Output;
    fn hash(&self) -> Self::Output;
}

impl Hashable for PublicKey {
    type Output = [u8; 32];

    fn hash(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};
        let mut hasher = Keccak256::new();
        hasher.update(self.as_bytes());
        hasher.finalize().into()
    }
}

#[derive(Debug)]
pub struct Ed25519Provider {
    private_key: PrivateKey,
}

impl Ed25519Provider {
    pub fn new(private_key: PrivateKey) -> Self {
        Self { private_key }
    }

    pub fn private_key(&self) -> &PrivateKey {
        &self.private_key
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        self.private_key.sign(data)
    }

    pub fn verify(&self, data: &[u8], signature: &Signature, public_key: &PublicKey) -> bool {
        public_key.verify(data, signature).is_ok()
    }
}

#[async_trait]
impl SigningProvider<TestContext> for Ed25519Provider {
    async fn sign_vote(&self, vote: Vote) -> Result<SignedVote<TestContext>, Error> {
        let signature = self.sign(&vote.to_sign_bytes());
        Ok(SignedVote::new(vote, signature))
    }

    async fn verify_signed_vote(
        &self,
        vote: &Vote,
        signature: &Signature,
        public_key: &PublicKey,
    ) -> Result<VerificationResult, Error> {
        Ok(VerificationResult::from_bool(
            public_key.verify(&vote.to_sign_bytes(), signature).is_ok(),
        ))
    }

    async fn sign_proposal(
        &self,
        proposal: Proposal,
    ) -> Result<SignedProposal<TestContext>, Error> {
        let signature = self.private_key.sign(&proposal.to_sign_bytes());
        Ok(SignedProposal::new(proposal, signature))
    }

    async fn verify_signed_proposal(
        &self,
        proposal: &Proposal,
        signature: &Signature,
        public_key: &PublicKey,
    ) -> Result<VerificationResult, Error> {
        Ok(VerificationResult::from_bool(
            public_key
                .verify(&proposal.to_sign_bytes(), signature)
                .is_ok(),
        ))
    }

    async fn sign_proposal_part(
        &self,
        proposal_part: ProposalPart,
    ) -> Result<SignedProposalPart<TestContext>, Error> {
        let signature = self.private_key.sign(&proposal_part.to_sign_bytes());
        Ok(SignedProposalPart::new(proposal_part, signature))
    }

    async fn verify_signed_proposal_part(
        &self,
        proposal_part: &ProposalPart,
        signature: &Signature,
        public_key: &PublicKey,
    ) -> Result<VerificationResult, Error> {
        Ok(VerificationResult::from_bool(
            public_key
                .verify(&proposal_part.to_sign_bytes(), signature)
                .is_ok(),
        ))
    }

    async fn sign_vote_extension(
        &self,
        extension: Bytes,
    ) -> Result<SignedExtension<TestContext>, Error> {
        let signature = self.private_key.sign(extension.as_ref());
        Ok(malachitebft_core_types::SignedMessage::new(
            extension, signature,
        ))
    }

    async fn verify_signed_vote_extension(
        &self,
        extension: &Bytes,
        signature: &Signature,
        public_key: &PublicKey,
    ) -> Result<VerificationResult, Error> {
        Ok(VerificationResult::from_bool(
            public_key.verify(extension.as_ref(), signature).is_ok(),
        ))
    }
}
