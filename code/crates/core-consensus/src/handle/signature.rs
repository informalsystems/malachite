use crate::prelude::*;

use crate::types::ConsensusMsg;
use crate::handle::vote::verify_vote_extension;
use std::collections::BTreeSet;

pub async fn verify_signature<Ctx>(
    co: &Co<Ctx>,
    signed_msg: SignedMessage<Ctx, ConsensusMsg<Ctx>>,
    validator: &Ctx::Validator,
) -> Result<bool, Error<Ctx>>
where
    Ctx: Context,
{
    let valid = perform!(co,
        Effect::VerifySignature(signed_msg, validator.public_key().clone(), Default::default()),
        Resume::SignatureValidity(valid) => valid
    );

    Ok(valid)
}

pub async fn sign_vote<Ctx>(co: &Co<Ctx>, vote: Ctx::Vote) -> Result<SignedVote<Ctx>, Error<Ctx>>
where
    Ctx: Context,
{
    let signed_vote = perform!(co,
        Effect::SignVote(vote, Default::default()),
        Resume::SignedVote(signed_vote) => signed_vote
    );

    Ok(signed_vote)
}

pub async fn sign_proposal<Ctx>(
    co: &Co<Ctx>,
    proposal: Ctx::Proposal,
) -> Result<SignedProposal<Ctx>, Error<Ctx>>
where
    Ctx: Context,
{
    let signed_proposal = perform!(co,
        Effect::SignProposal(proposal, Default::default()),
        Resume::SignedProposal(signed_proposal) => signed_proposal
    );

    Ok(signed_proposal)
}

pub async fn verify_commit_certificate<Ctx>(
    co: &Co<Ctx>,
    certificate: CommitCertificate<Ctx>,
    validator_set: Ctx::ValidatorSet,
    threshold_params: ThresholdParams,
) -> Result<Result<(), CertificateError<Ctx>>, Error<Ctx>>
where
    Ctx: Context,
{
    let result = perform!(co,
        Effect::VerifyCommitCertificate(certificate, validator_set, threshold_params, Default::default()),
        Resume::CertificateValidity(result) => result
    );

    Ok(result)
}

pub async fn verify_round_certificate<Ctx>(
    co: &Co<Ctx>,
    certificate: RoundCertificate<Ctx>,
    validator_set: Ctx::ValidatorSet,
    threshold_params: ThresholdParams,
) -> Result<Result<(), CertificateError<Ctx>>, Error<Ctx>>
where
    Ctx: Context,
{
    let result = perform!(co,
        Effect::VerifyRoundCertificate(certificate, validator_set, threshold_params, Default::default()),
        Resume::CertificateValidity(result) => result
    );

    Ok(result)
}

/// Verify all vote signatures and extensions in a prevote certificate.
///
/// This validates that:
/// 1. All votes are for the expected height
/// 2. No duplicate validators (prevents weight inflation attacks)
/// 3. All votes are Prevote type (FaB only has prevotes)
/// 4. All validators are in the validator set
/// 5. All vote signatures are cryptographically valid
/// 6. All vote extensions are valid (if present)
///
/// Returns Ok(true) if all checks pass, Ok(false) if any check fails.
/// Returns Err only for internal errors (not validation failures).
///
/// ## Security
/// This prevents Byzantine nodes from forging certificates with fake signatures,
/// which could violate FaB safety guarantees and cause agreement violations.
pub async fn verify_prevote_certificate<Ctx>(
    co: &Co<Ctx>,
    state: &State<Ctx>,
    certificate: &[SignedVote<Ctx>],
    validator_set: &Ctx::ValidatorSet,
    expected_height: Ctx::Height,
) -> Result<bool, Error<Ctx>>
where
    Ctx: Context,
{
    // Track seen validators to detect duplicates
    let mut seen_validators = BTreeSet::new();

    // Validate each vote in the certificate
    // Ordered from fast checks to slow checks (fail fast)
    for vote in certificate {
        let validator_address = vote.validator_address();

        // 1. Height check (FAST - just comparison)
        if vote.height() != expected_height {
            warn!(
                validator = %validator_address,
                vote_height = %vote.height(),
                expected_height = %expected_height,
                "Certificate contains vote for wrong height"
            );
            return Ok(false);
        }

        // 2. Duplicate check (FAST - BTreeSet insert)
        // Prevents weight inflation attacks where Byzantine includes same vote multiple times
        if !seen_validators.insert(validator_address.clone()) {
            warn!(
                validator = %validator_address,
                "Certificate contains duplicate vote from same validator"
            );
            return Ok(false);
        }

        // 3. Vote type check (FAST - enum comparison)
        // FaB only has Prevote votes, no Precommits
        if vote.vote_type() != VoteType::Prevote {
            warn!(
                validator = %validator_address,
                vote_type = ?vote.vote_type(),
                "Certificate contains non-Prevote vote"
            );
            return Ok(false);
        }

        // 4. Validator exists check (FAST - hash lookup)
        let Some(validator) = validator_set.get_by_address(validator_address) else {
            warn!(
                validator = %validator_address,
                "Certificate contains vote from unknown validator"
            );
            return Ok(false);
        };

        // 5. Signature validation (SLOW - Ed25519 crypto operation)
        let signed_msg = vote.clone().map(ConsensusMsg::Vote);
        if !verify_signature(co, signed_msg, validator).await? {
            warn!(
                validator = %validator_address,
                vote_height = %vote.height(),
                vote_round = %vote.round(),
                "Certificate contains vote with invalid signature"
            );
            return Ok(false);
        }

        // 6. Extension validation (SLOW - application call)
        // Extensions contain application-specific data that must be validated
        if !verify_vote_extension(co, state, vote, validator).await? {
            warn!(
                validator = %validator_address,
                vote_height = %vote.height(),
                vote_round = %vote.round(),
                "Certificate contains vote with invalid extension"
            );
            return Ok(false);
        }
    }

    // All votes valid
    Ok(true)
}
