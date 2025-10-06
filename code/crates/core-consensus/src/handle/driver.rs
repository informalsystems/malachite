use malachitebft_core_driver::Input as DriverInput;
use malachitebft_core_driver::Output as DriverOutput;

use crate::handle::decide::decide;
use crate::handle::on_proposal;
use crate::handle::signature::sign_proposal;
use crate::handle::signature::sign_vote;
use crate::handle::vote::on_vote;
// FaB: Removed HIDDEN_LOCK_ROUND import - not used in FaB
use crate::prelude::*;
use crate::types::{
    LivenessMsg, {LocallyProposedValue, SignedConsensusMsg},
};
use crate::util::pretty::PrettyVal;
use crate::Role;

use super::propose::on_propose;

#[async_recursion]
pub async fn apply_driver_input<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    input: DriverInput<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    match &input {
        DriverInput::NewRound(height, round, proposer) => {
            #[cfg(feature = "metrics")]
            metrics.round.set(round.as_i64());

            // Publishing the round certificate upon entering round > 0
            // is part of the new round synchronization mechanism, which
            // ensures all validators advance through rounds even in the
            // presence of asynchrony or Byzantine behavior. Moreover,
            // it guarantees that after GST, all correct replicas will receive
            // the round certificate and enter the same round within bounded time.
            if round > &Round::new(0) {
                if let Some(cert) = state.driver.round_certificate() {
                    if cert.enter_round == *round {
                        info!(
                            %cert.certificate.height,
                            %cert.enter_round,
                            number_of_votes = cert.certificate.round_signatures.len(),
                            "Sending round certificate"
                        );
                        perform!(
                            co,
                            Effect::PublishLivenessMsg(
                                LivenessMsg::SkipRoundCertificate(cert.certificate.clone()),
                                Default::default()
                            )
                        );
                    }
                }
            }

            let role = if state.address() == proposer {
                Role::Proposer
            } else if state.is_validator() {
                Role::Validator
            } else {
                Role::None
            };

            info!(%height, %round, %proposer, ?role, "Starting new round");

            state.last_signed_prevote = None;
            state.last_signed_precommit = None;

            perform!(co, Effect::CancelAllTimeouts(Default::default()));
            perform!(
                co,
                Effect::StartRound(*height, *round, proposer.clone(), role, Default::default())
            );

            #[cfg(feature = "metrics")]
            metrics.rebroadcast_timeouts.inc();

            // Schedule rebroadcast timer
            let timeout = Timeout::rebroadcast(*round);
            perform!(co, Effect::ScheduleTimeout(timeout, Default::default()));
        }

        DriverInput::ProposeValue(round, _) => {
            perform!(
                co,
                Effect::CancelTimeout(Timeout::propose(*round), Default::default())
            );
        }

        DriverInput::Proposal(proposal, _validity) => {
            if proposal.height() != state.driver.height() {
                warn!(
                    "Ignoring proposal for height {}, current height: {}",
                    proposal.height(),
                    state.driver.height()
                );

                return Ok(());
            }
        }

        DriverInput::Vote(vote) => {
            if vote.height() != state.driver.height() {
                warn!(
                    "Ignoring vote for height {}, current height: {}",
                    vote.height(),
                    state.driver.height()
                );

                return Ok(());
            }
        }

        // FaB: Removed CommitCertificate - no commit certificates in FaB
        // FaB: Removed PolkaCertificate - no polka certificates in FaB

        DriverInput::ReceiveDecision(_value, _certificate) => {
            // FaB: ReceiveDecision doesn't need height validation here
            // FaB: The state machine will handle validation
        }

        DriverInput::TimeoutElapsed(_) => (),
    }

    // Record the step we were in
    let prev_step = state.driver.step();

    let outputs = state
        .driver
        .process(input)
        .map_err(|e| Error::DriverProcess(e))?;

    // Record the step we are now at
    let new_step = state.driver.step();

    // If the step has changed, update the metrics
    if prev_step != new_step {
        debug!(step.previous = ?prev_step, step.new = ?new_step, "Transitioned to new step");

        // FaB: prevoted_value() returns Option<&Value> (no round info)
        // FaB: Log if we have a prevoted value when entering propose step
        if let Some(_prevoted) = state.driver.prevoted_value() {
            if state.driver.step_is_propose() {
                info!("Entering Propose step with a prevoted value");
            }
        }

        #[cfg(feature = "metrics")]
        {
            metrics.step_end(prev_step);
            metrics.step_start(new_step);
        }
    }

    if prev_step != new_step && state.driver.step_is_prevote() {
        // Cancel the Propose timeout since we have moved from Propose to Prevote
        perform!(
            co,
            Effect::CancelTimeout(Timeout::propose(state.driver.round()), Default::default())
        );
    }

    process_driver_outputs(co, state, metrics, outputs).await?;

    Ok(())
}

async fn process_driver_outputs<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    outputs: Vec<DriverOutput<Ctx>>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    for output in outputs {
        process_driver_output(co, state, metrics, output).await?;
    }

    Ok(())
}

async fn process_driver_output<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    output: DriverOutput<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    match output {
        DriverOutput::NewRound(height, round) => {
            let proposer = state.get_proposer(height, round);
            apply_driver_input(
                co,
                state,
                metrics,
                DriverInput::NewRound(height, round, proposer.clone()),
            )
            .await
        }

        DriverOutput::Propose(proposal) => {
            info!(
                id = %proposal.value().id(),
                round = %proposal.round(),
                "Proposing value"
            );

            // Only sign and publish if we're in the validator set
            if state.is_validator() {
                let signed_proposal = sign_proposal(co, proposal.clone()).await?;

                if signed_proposal.pol_round().is_defined() {
                    perform!(
                        co,
                        Effect::RestreamProposal(
                            signed_proposal.height(),
                            signed_proposal.round(),
                            signed_proposal.pol_round(),
                            signed_proposal.validator_address().clone(),
                            signed_proposal.value().id(),
                            Default::default()
                        )
                    );
                }

                on_proposal(co, state, metrics, signed_proposal.clone()).await?;

                // Proposal messages should not be broadcasted if they are implicit,
                // instead they should be inferred from the block parts.
                if state.params.value_payload.include_proposal() {
                    perform!(
                        co,
                        Effect::PublishConsensusMsg(
                            SignedConsensusMsg::Proposal(signed_proposal),
                            Default::default()
                        )
                    );
                };

                // FaB: Removed polka certificate broadcasting
                // FaB: In FaB, there are no polka certificates (2f+1 prevotes)
                // FaB: Validators receive proposals and prevotes directly
            }

            Ok(())
        }

        DriverOutput::Vote(vote) => {
            // FaB: In FaB, when casting a prevote for a value, prune old votes
            // FaB: No HIDDEN_LOCK_ROUND mechanism - liveness is ensured through:
            // FaB: 1. Round skipping (f+1 prevotes from higher round)
            // FaB: 2. Periodic rebroadcast of lastPrevote and proposal (lines 111-113 of pseudocode)
            if let (VoteType::Prevote, NilOrVal::Val(_value_id)) = (vote.vote_type(), vote.value())
            {
                // Prune all votes and certificates for the previous rounds
                state.driver.prune_votes_and_certificates(vote.round());
            }

            if state.is_validator() {
                info!(
                    vote_type = ?vote.vote_type(),
                    value = %PrettyVal(vote.value().as_ref()),
                    round = %vote.round(),
                    "Voting",
                );

                let extended_vote = extend_vote(co, vote).await?;
                let signed_vote = sign_vote(co, extended_vote).await?;

                on_vote(co, state, metrics, signed_vote.clone()).await?;

                perform!(
                    co,
                    Effect::PublishConsensusMsg(
                        SignedConsensusMsg::Vote(signed_vote.clone()),
                        Default::default()
                    )
                );

                state.set_last_vote(signed_vote);

                // Schedule rebroadcast timer
                let timeout = Timeout::rebroadcast(state.driver.round());
                perform!(co, Effect::ScheduleTimeout(timeout, Default::default()));
            }

            Ok(())
        }

        // FaB: Decide now includes certificate
        DriverOutput::Decide(consensus_round, proposal, certificate) => {
            info!(
                round = %consensus_round,
                height = %proposal.height(),
                value = %proposal.value().id(),
                "Decided",
            );

            // FaB: Pass certificate to decide handler
            decide(co, state, metrics, certificate).await?;

            Ok(())
        }

        DriverOutput::ScheduleTimeout(timeout) => {
            info!(round = %timeout.round, step = ?timeout.kind, "Scheduling timeout");

            perform!(co, Effect::ScheduleTimeout(timeout, Default::default()));

            Ok(())
        }

        DriverOutput::GetValue(height, round, timeout) => {
            if let Some(full_proposal) =
                state.full_proposal_at_round_and_proposer(&height, round, state.address())
            {
                info!(%height, %round, "Using already existing value");

                let local_value = LocallyProposedValue {
                    height: full_proposal.proposal.height(),
                    round: full_proposal.proposal.round(),
                    value: full_proposal.builder_value.clone(),
                };

                on_propose(co, state, metrics, local_value).await?;
            } else {
                info!(%height, %round, "Requesting value from application");

                perform!(
                    co,
                    Effect::GetValue(height, round, timeout, Default::default())
                );
            }

            Ok(())
        }
    }
}

async fn extend_vote<Ctx: Context>(co: &Co<Ctx>, vote: Ctx::Vote) -> Result<Ctx::Vote, Error<Ctx>> {
    // FaB: Changed from Precommit to Prevote (only prevotes in FaB)
    let VoteType::Prevote = vote.vote_type() else {
        return Ok(vote);
    };

    let NilOrVal::Val(value_id) = vote.value().as_ref().cloned() else {
        return Ok(vote);
    };

    let extension = perform!(
        co,


        Effect::ExtendVote(vote.height(), vote.round(), value_id, Default::default()),
        Resume::VoteExtension(extension) => extension);

    if let Some(extension) = extension {
        Ok(vote.extend(extension))
    } else {
        Ok(vote)
    }
}
