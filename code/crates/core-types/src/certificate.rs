use alloc::string::ToString;
use alloc::vec::Vec;
use bytes::Bytes;
use derive_where::derive_where;
use malachitebft_peer::PeerId;
use thiserror::Error;

use crate::{
    BoxError, Context, NilOrVal, Round, Signature, SignedVote, ValueId, Vote, VoteType, VotingPower,
};

/// Represents a signature for a commit certificate, with the address of the validator that produced it.
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct CommitSignature<Ctx: Context> {
    /// The address associated with the signature.
    pub address: Ctx::Address,
    /// The signature itself.
    pub signature: Signature<Ctx>,
}

impl<Ctx: Context> CommitSignature<Ctx> {
    /// Create a new `CommitSignature` from an address and a signature.
    pub fn new(address: Ctx::Address, signature: Signature<Ctx>) -> Self {
        Self { address, signature }
    }
}

/// Represents a certificate containing the message (height, round, value_id) and the commit signatures.
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct CommitCertificate<Ctx: Context> {
    /// The height of the certificate.
    pub height: Ctx::Height,
    /// The round number associated with the certificate.
    pub round: Round,
    /// The identifier for the value being certified.
    pub value_id: ValueId<Ctx>,
    /// A vector of signatures that make up the certificate.
    pub commit_signatures: Vec<CommitSignature<Ctx>>,
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
        let commit_signatures = commits
            .into_iter()
            .filter(|vote| {
                matches!(vote.value(), NilOrVal::Val(id) if id == &value_id)
                    && vote.vote_type() == VoteType::Precommit
                    && vote.round() == round
                    && vote.height() == height
            })
            .map(|signed_vote| {
                CommitSignature::new(
                    signed_vote.validator_address().clone(),
                    signed_vote.signature,
                )
            })
            .collect();

        Self {
            height,
            round,
            value_id,
            commit_signatures,
        }
    }
}

/// Represents a signature for a polka certificate, with the address of the validator that produced it.
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct PolkaSignature<Ctx: Context> {
    /// The address associated with the signature.
    pub address: Ctx::Address,
    /// The signature itself.
    pub signature: Signature<Ctx>,
}

impl<Ctx: Context> PolkaSignature<Ctx> {
    /// Create a new `CommitSignature` from an address and a signature.
    pub fn new(address: Ctx::Address, signature: Signature<Ctx>) -> Self {
        Self { address, signature }
    }
}

/// Represents a certificate witnessing a Polka at a given height and round.
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct PolkaCertificate<Ctx: Context> {
    /// The height at which a Polka was witnessed
    pub height: Ctx::Height,
    /// The round at which a Polka that was witnessed
    pub round: Round,
    /// The value that the Polka is for
    pub value_id: ValueId<Ctx>,
    /// The signatures for the votes that make up the Polka
    pub polka_signatures: Vec<PolkaSignature<Ctx>>,
}

impl<Ctx: Context> PolkaCertificate<Ctx> {
    /// Creates a new `PolkaCertificate` from signed prevotes.
    pub fn new(
        height: Ctx::Height,
        round: Round,
        value_id: ValueId<Ctx>,
        votes: Vec<SignedVote<Ctx>>,
    ) -> Self {
        // Collect all polka signatures from the signed votes
        let polka_signatures = votes
            .into_iter()
            .filter(|vote| {
                matches!(vote.value(), NilOrVal::Val(id) if id == &value_id)
                    && vote.vote_type() == VoteType::Prevote
                    && vote.round() == round
                    && vote.height() == height
            })
            .map(|signed_vote| {
                PolkaSignature::new(
                    signed_vote.validator_address().clone(),
                    signed_vote.signature,
                )
            })
            .collect();

        Self {
            height,
            round,
            value_id,
            polka_signatures,
        }
    }
}

/// Represents an error that can occur when verifying a certificate.
#[derive(Error)]
#[derive_where(Debug, PartialEq)]
pub enum CertificateError<Ctx: Context> {
    /// One of the commit signatures is invalid.
    #[error("Invalid commit signature: {0:?}")]
    InvalidCommitSignature(CommitSignature<Ctx>),

    /// One of the commit signatures is invalid.
    #[error("Invalid polka signature: {0:?}")]
    InvalidPolkaSignature(PolkaSignature<Ctx>),

    /// One of the round signatures is invalid.
    #[error("Invalid round signature: {0:?}")]
    InvalidRoundSignature(RoundSignature<Ctx>),

    /// A validator in the certificate is not in the validator set.
    #[error("A validator in the certificate is not in the validator set: {0:?}")]
    UnknownValidator(Ctx::Address),

    /// Not enough voting power has signed the certificate.
    #[error(
        "Not enough voting power has signed the certificate: \
         signed={signed}, total={total}, expected={expected}"
    )]
    NotEnoughVotingPower {
        /// Signed voting power
        signed: VotingPower,
        /// Total voting power
        total: VotingPower,
        /// Expected voting power
        expected: VotingPower,
    },

    /// Multiple votes from the same validator.
    #[error("Multiple votes from the same validator: {0}")]
    DuplicateVote(Ctx::Address),

    /// A Prevote was incorrectly included in a Precommit round certificate.
    #[error("Prevote received in precommit round certificate from validator: {0}")]
    InvalidVoteType(Ctx::Address),

    /// An error occurred while verifying the certificate.
    #[error("Signature verification error: {}", .0.as_ref().map(|e| e.to_string()).unwrap_or_default())]
    VerificationError(Option<BoxError>),
}

/// Represents a signature for a round certificate, with the address of the validator that produced it.
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct RoundSignature<Ctx: Context> {
    /// The vote type
    pub vote_type: VoteType,
    /// The value id
    pub value_id: NilOrVal<ValueId<Ctx>>,
    /// The address associated with the signature.
    pub address: Ctx::Address,
    /// The signature itself.
    pub signature: Signature<Ctx>,
}

impl<Ctx: Context> RoundSignature<Ctx> {
    /// Create a new `CommitSignature` from an address and a signature.
    pub fn new(
        vote_type: VoteType,
        value_id: NilOrVal<ValueId<Ctx>>,
        address: Ctx::Address,
        signature: Signature<Ctx>,
    ) -> Self {
        Self {
            vote_type,
            value_id,
            address,
            signature,
        }
    }
}

/// Describes the type of a `RoundCertificate`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "borsh",
    derive(::borsh::BorshSerialize, ::borsh::BorshDeserialize)
)]
pub enum RoundCertificateType {
    /// Composed of f+1 votes (e.g., SkipRound)
    Skip,
    /// Composed of 2f+1 Precommit votes from the previous round (e.g., PrecommitAny)
    Precommit,
}

/// Represents a certificate used to justify entering a new round at a given height.
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct RoundCertificate<Ctx: Context> {
    /// The height at which a certificate was witnessed
    pub height: Ctx::Height,
    /// The round of the votes that made up the certificate
    pub round: Round,
    /// The type of the certificate
    pub cert_type: RoundCertificateType,
    /// The signatures for the votes that make up the certificate
    pub round_signatures: Vec<RoundSignature<Ctx>>,
}

impl<Ctx: Context> RoundCertificate<Ctx> {
    /// Creates a new `RoundCertificate` from a vector of signed votes.
    pub fn new_from_votes(
        height: Ctx::Height,
        round: Round,
        cert_type: RoundCertificateType,
        votes: Vec<SignedVote<Ctx>>,
    ) -> Self {
        RoundCertificate {
            height,
            round,
            cert_type,
            round_signatures: votes
                .into_iter()
                .map(|v| {
                    RoundSignature::new(
                        v.vote_type(),
                        v.value().clone(),
                        v.validator_address().clone(),
                        v.signature,
                    )
                })
                .collect(),
        }
    }
}

/// Represents a local certificate that triggered or will trigger the start of a new round.
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct EnterRoundCertificate<Ctx: Context> {
    /// The certificate that triggered or will trigger the start of a new round
    pub certificate: RoundCertificate<Ctx>,
    /// The round that will be entered due to the `RoundCertificate`.
    /// - If the certificate is `PrecommitAny`, it contains signatures from the previous round,
    ///   so `enter_round` will be one more than the round of those signatures.
    /// - If the certificate is `SkipRound`, it contains signatures from the round being entered,
    ///   so `enter_round` will be equal to the round of those signatures.
    pub enter_round: Round,
}

impl<Ctx: Context> EnterRoundCertificate<Ctx> {
    /// Creates a new `LocalRoundCertificate` from a vector of signed votes.
    pub fn new_from_votes(
        height: Ctx::Height,
        enter_round: Round,
        round: Round,
        cert_type: RoundCertificateType,
        votes: Vec<SignedVote<Ctx>>,
    ) -> Self {
        Self {
            certificate: RoundCertificate::new_from_votes(height, round, cert_type, votes),
            enter_round,
        }
    }
}

/// Represents a response to a value request.
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct ValueResponse<Ctx: Context> {
    /// The peer that sent the value response
    pub peer: PeerId,
    /// The raw bytes of the value
    pub value_bytes: Bytes,
    /// The commit certificate proving the value was decided
    pub certificate: CommitCertificate<Ctx>,
}

impl<Ctx: Context> ValueResponse<Ctx> {
    /// Creates a new `ValueResponse` from the raw bytes of the value and the commit certificate.
    pub fn new(peer: PeerId, value_bytes: Bytes, certificate: CommitCertificate<Ctx>) -> Self {
        Self {
            peer,
            value_bytes,
            certificate,
        }
    }
}
