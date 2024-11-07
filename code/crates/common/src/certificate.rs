use core::fmt;

use alloc::vec::Vec;
use derive_where::derive_where;

use crate::{
    Context, Extension, NilOrVal, Round, Signature, SignedVote, Validator, ValidatorSet, ValueId,
    Vote, VoteType, VotingPower,
};

/// Represents a signature for a certificate, including the address and the signature itself.
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct CommitSignature<Ctx: Context> {
    /// The address associated with the signature.
    pub address: Ctx::Address,
    /// The signature itself.
    pub signature: Signature<Ctx>,
    /// Vote extension
    /// TODO - add extension signature
    pub extension: Option<Extension>,
}

/// Aggregated signature.
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct AggregatedSignature<Ctx: Context> {
    /// A collection of commit signatures.
    pub signatures: Vec<CommitSignature<Ctx>>,
}

impl<Ctx: Context> AggregatedSignature<Ctx> {
    /// Create a new `AggregatedSignature` from a vector of commit signatures.
    pub fn new(signatures: Vec<CommitSignature<Ctx>>) -> Self {
        Self { signatures }
    }
}

/// Represents a certificate containing the message (height, round, value_id) and an aggregated signature.
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct CommitCertificate<Ctx: Context> {
    /// The height of the certificate.
    pub height: Ctx::Height,
    /// The round number associated with the certificate.
    pub round: Round,
    /// The identifier for the value being certified.
    pub value_id: ValueId<Ctx>,
    /// A vector of signatures that make up the certificate.
    pub aggregated_signature: AggregatedSignature<Ctx>, // TODO - type in context
}

impl<Ctx: Context> CommitCertificate<Ctx> {
    /// Creates a new `CommitCertificate` from a vector of signed votes.
    pub fn new(
        height: Ctx::Height,
        round: Round,
        value_id: ValueId<Ctx>,
        commits: Vec<SignedVote<Ctx>>,
    ) -> Self {
        // Collect all commit signatures from the signed votes
        let signatures = commits
            .into_iter()
            .filter(|vote| {
                matches!(vote.value(), NilOrVal::Val(id) if id == &value_id)
                    && vote.vote_type() == VoteType::Precommit
                    && vote.round() == round
                    && vote.height() == height
            })
            .map(|signed_vote| CommitSignature {
                address: signed_vote.validator_address().clone(),
                signature: signed_vote.signature,
                extension: signed_vote.message.extension().cloned(),
            })
            .collect();

        // Create the aggregated signature
        let aggregated_signature = AggregatedSignature::new(signatures);

        Self {
            height,
            round,
            value_id,
            aggregated_signature,
        }
    }

    /// Verify the certificate against the given validator set.
    ///
    /// - Check that we have 2/3+ of voting power has signed the certificate
    /// - For each commit signature in the certificate:
    ///   - Reconstruct the signed precommit and verify its signature
    ///
    /// If any of those steps fail, return false.
    ///
    /// TODO: Move to Context
    pub fn verify(
        &self,
        ctx: &Ctx,
        validator_set: &Ctx::ValidatorSet,
    ) -> Result<(), CertificateError<Ctx>> {
        // 1. Check that we have 2/3+ of voting power has signed the certificate
        let total_voting_power = validator_set.total_voting_power();
        let mut signed_voting_power = 0;

        // 2. For each commit signature, reconstruct the signed precommit and verify the signature
        for commit_sig in &self.aggregated_signature.signatures {
            // Skip if validator not in set
            // TODO: Should we emit an error here instead of skipping that signature?
            let validator = match validator_set.get_by_address(&commit_sig.address) {
                Some(validator) => validator,
                None => continue,
            };

            // Reconstruct the vote that was signed
            let vote = Ctx::new_precommit(
                self.height,
                self.round,
                NilOrVal::Val(self.value_id.clone()),
                validator.address().clone(),
            );

            // Verify signature
            if !ctx.verify_signed_vote(&vote, &commit_sig.signature, validator.public_key()) {
                return Err(CertificateError::InvalidCommitSignature(commit_sig.clone()));
            }

            signed_voting_power += validator.voting_power();
        }

        // Check if we have 2/3+ voting power
        // TODO: Should this use the `ThresholdParams` instead of being hardcoded to 2/3+?
        if signed_voting_power * 3 > total_voting_power * 2 {
            Ok(())
        } else {
            Err(CertificateError::NotEnoughVotingPower {
                signed: signed_voting_power,
                total: total_voting_power,
                expected: 2 * total_voting_power / 3,
            })
        }
    }
}

/// Represents an error that can occur when verifying a certificate.
#[derive_where(Clone, Debug)]
pub enum CertificateError<Ctx: Context> {
    /// One of the commit signature is invalid.
    InvalidCommitSignature(CommitSignature<Ctx>),

    /// Not enough voting power has signed the certificate.
    NotEnoughVotingPower {
        /// Signed voting power
        signed: VotingPower,
        /// Total voting power
        total: VotingPower,
        /// Expected voting power
        expected: VotingPower,
    },
}

impl<Ctx: Context> fmt::Display for CertificateError<Ctx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CertificateError::InvalidCommitSignature(commit_sig) => {
                write!(f, "Invalid commit signature: {commit_sig:?}")
            }

            CertificateError::NotEnoughVotingPower {
                signed,
                total,
                expected,
            } => {
                write!(
                    f,
                    "Not enough voting power has signed the certificate: \
                     signed={signed}, total={total}, expected={expected}",
                )
            }
        }
    }
}
