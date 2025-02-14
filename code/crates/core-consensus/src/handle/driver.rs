use malachitebft_core_driver::Input as DriverInput;
use malachitebft_core_driver::Output as DriverOutput;

use crate::handle::on_proposal;
use crate::handle::signature::sign_proposal;
use crate::handle::signature::sign_vote;
use crate::handle::vote::on_vote;
use crate::prelude::*;
use crate::types::SignedConsensusMsg;
use crate::util::pretty::PrettyVal;

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

            info!(%height, %round, %proposer, "Starting new round");

            perform!(co, Effect::CancelAllTimeouts(Default::default()));
            perform!(
                co,
                Effect::StartRound(*height, *round, proposer.clone(), Default::default())
            );
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

            perform!(
                co,
                Effect::CancelTimeout(Timeout::propose(proposal.round()), Default::default())
            );
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

            // TODO: this is a hack to prevent the node from sending the same vote twice
            // it doesn't cover multiple rounds.
            // WAL replaying our own vote
            if state.check_vote_against_last_sent(vote) {
                state.set_last_sent_vote(vote.clone());
            }
        }

        DriverInput::CommitCertificate(certificate) => {
            if certificate.height != state.driver.height() {
                warn!(
                    "Ignoring certificate for height {}, current height: {}",
                    certificate.height,
                    state.driver.height()
                );

                return Ok(());
            }
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

        if let Some(valid) = &state.driver.valid_value() {
            if state.driver.step_is_propose() {
                info!(
                    round = %valid.round,
                    "Entering Propose step with a valid value"
                );
            }
        }

        #[cfg(feature = "metrics")]
        {
            metrics.step_end(prev_step);
            metrics.step_start(new_step);
        }
    }

    if prev_step != new_step {
        if state.driver.step_is_prevote() {
            perform!(
                co,
                Effect::ScheduleTimeout(
                    Timeout::prevote_time_limit(state.driver.round()),
                    Default::default()
                )
            );
        }

        if state.driver.step_is_precommit() {
            perform!(
                co,
                Effect::CancelTimeout(
                    Timeout::prevote_time_limit(state.driver.round()),
                    Default::default()
                )
            );
            perform!(
                co,
                Effect::ScheduleTimeout(
                    Timeout::precommit_time_limit(state.driver.round()),
                    Default::default()
                )
            );
        }

        if state.driver.step_is_commit() {
            perform!(
                co,
                Effect::CancelTimeout(
                    Timeout::precommit_time_limit(state.driver.round()),
                    Default::default()
                )
            );
        }
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
                let signed_proposal = sign_proposal(co, proposal).await?;

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
                        Effect::Publish(
                            SignedConsensusMsg::Proposal(signed_proposal),
                            Default::default()
                        )
                    );
                };
            }

            Ok(())
        }

        DriverOutput::Vote(vote) => {
            info!(
                vote_type = ?vote.vote_type(),
                value = %PrettyVal(vote.value().as_ref()),
                round = %vote.round(),
                "Voting",
            );

            let extended_vote = extend_vote(co, vote).await?;
            let signed_vote = sign_vote(co, extended_vote).await?;

            // Only sign and publish if we're in the validator set
            if state.is_validator() && state.check_vote_against_last_sent(&signed_vote) {
                on_vote(co, state, metrics, signed_vote.clone()).await?;

                perform!(
                    co,
                    Effect::Publish(
                        SignedConsensusMsg::Vote(signed_vote.clone()),
                        Default::default()
                    )
                );

                let timeout = match signed_vote.vote_type() {
                    VoteType::Prevote => Timeout::prevote_rebroadcast(state.driver.round()),
                    VoteType::Precommit => Timeout::precommit_rebroadcast(state.driver.round()),
                };

                state.set_last_sent_vote(signed_vote);

                perform!(co, Effect::ScheduleTimeout(timeout, Default::default()));
            } else {
                warn!("YYY - Vote is NOT the same as the last sent vote, ignoring");
            }

            Ok(())
        }

        DriverOutput::Decide(consensus_round, proposal) => {
            info!(
                round = %consensus_round,
                height = %proposal.height(),
                value = %proposal.value().id(),
                "Decided",
            );

            // Store value decided on for retrieval when timeout commit elapses
            state.store_decision(state.driver.height(), consensus_round, proposal.clone());

            perform!(
                co,
                Effect::ScheduleTimeout(Timeout::commit(consensus_round), Default::default())
            );

            Ok(())
        }

        DriverOutput::ScheduleTimeout(timeout) => {
            info!(round = %timeout.round, step = ?timeout.kind, "Scheduling timeout");

            perform!(co, Effect::ScheduleTimeout(timeout, Default::default()));

            Ok(())
        }

        DriverOutput::GetValue(height, round, timeout) => {
            info!(%height, %round, "Requesting value");

            perform!(
                co,
                Effect::GetValue(height, round, timeout, Default::default())
            );

            Ok(())
        }
    }
}

async fn extend_vote<Ctx: Context>(co: &Co<Ctx>, vote: Ctx::Vote) -> Result<Ctx::Vote, Error<Ctx>> {
    let VoteType::Precommit = vote.vote_type() else {
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
