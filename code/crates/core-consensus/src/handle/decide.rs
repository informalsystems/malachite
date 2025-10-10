// FaB: Import Certificate from state machine
use malachitebft_core_state_machine::input::Certificate;
use crate::prelude::*;

#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub async fn decide<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    certificate: Certificate<Ctx>,  // FaB: Certificate comes from Decision output
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    assert!(state.driver.step_is_commit());

    let height = state.driver.height();
    let consensus_round = state.driver.round();

    let Some((proposal_round, decided_value)) = state.decided_value() else {
        return Err(Error::DecisionNotFound(height, consensus_round));
    };

    let decided_id = decided_value.id();

    // FaB: Certificate is already provided from the Decision output
    // FaB: It contains 4f+1 prevote messages that were already validated by the state machine
    // FaB: Extract vote extensions from the certificate
    let mut certificate_votes = certificate.clone();
    let extensions = extract_vote_extensions(&mut certificate_votes);

    let Some((proposal, _, _certificate)) = state
        .driver
        .proposal_and_validity_for_round_and_value(proposal_round, decided_id.clone())
    else {
        return Err(Error::DriverProposalNotFound(height, proposal_round));
    };

    let Some(full_proposal) =
        state.full_proposal_at_round_and_value(&height, proposal_round, &decided_value)
    else {
        return Err(Error::FullProposalNotFound(height, proposal_round));
    };

    if proposal.value().id() != decided_id {
        info!(
            "Decide: driver proposal value id {} does not match the decided value id {}, this may happen if consensus and value sync run in parallel",
            proposal.value().id(),
            decided_id
        );
    }

    assert_eq!(full_proposal.builder_value.id(), decided_id);
    assert_eq!(full_proposal.proposal.value().id(), decided_id);
    assert_eq!(full_proposal.validity, Validity::Valid);

    // FaB: All votes in this certificate have been cryptographically verified before being added to VoteKeeper.
    // FaB: Verification happens at entry points: on_vote() verifies gossip/sync votes, verify_prevote_certificate()
    // FaB: verifies proposal certificates, verify_round_certificate() verifies round certificates.
    // FaB: The state machine only checks thresholds (4f+1) and matching - it does NOT verify signatures.
    // FaB: This certificate was built from vote_keeper.latest_prevotes which maintains the invariant that
    // FaB: it only contains pre-verified votes. No re-verification is needed here.

    // Update metrics
    #[cfg(feature = "metrics")]
    {
        // We are only interested in consensus time for round 0, ie. in the happy path.
        if consensus_round == Round::new(0) {
            metrics.consensus_end();
        }

        metrics.block_end();

        metrics
            .consensus_round
            .observe(consensus_round.as_i64() as f64);

        metrics
            .proposal_round
            .observe(proposal_round.as_i64() as f64);
    }

    #[cfg(feature = "debug")]
    {
        for trace in state.driver.get_traces() {
            debug!(%trace, "Consensus trace");
        }
    }

    // FaB: Emit the Decide effect with the certificate and extracted extensions
    perform!(
        co,
        Effect::Decide(certificate_votes, extensions, Default::default())
    );

    Ok(())
}

// Extract vote extensions from a list of votes,
// removing them from each vote in the process.
pub fn extract_vote_extensions<Ctx: Context>(votes: &mut [SignedVote<Ctx>]) -> VoteExtensions<Ctx> {
    let extensions = votes
        .iter_mut()
        .filter_map(|vote| {
            vote.message
                .take_extension()
                .map(|e| (vote.validator_address().clone(), e))
        })
        .collect();

    VoteExtensions::new(extensions)
}
