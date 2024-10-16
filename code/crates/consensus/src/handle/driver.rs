use malachite_driver::Input as DriverInput;
use malachite_driver::Output as DriverOutput;

use crate::prelude::*;

use crate::handle::on_proposal;
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
            metrics.round.set(round.as_i64());

            info!(%height, %round, address = %proposer, "Starting new round");
            perform!(co, Effect::CancelAllTimeouts);
            perform!(co, Effect::StartRound(*height, *round, proposer.clone()));
        }

        DriverInput::ProposeValue(round, _) => {
            perform!(co, Effect::CancelTimeout(Timeout::propose(*round)));
        }

        DriverInput::Proposal(proposal, validity) => {
            if proposal.height() != state.driver.height() {
                warn!(
                    "Ignoring proposal for height {}, current height: {}",
                    proposal.height(),
                    state.driver.height()
                );

                return Ok(());
            }

            // Store the proposal
            state
                .driver
                .proposal_keeper
                .apply_proposal(proposal.clone(), *validity);

            perform!(
                co,
                Effect::CancelTimeout(Timeout::propose(proposal.round()))
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
        debug!("Transitioned from {prev_step:?} to {new_step:?}");
        if let Some(valid) = &state.driver.round_state.valid {
            if state.driver.step_is_propose() {
                info!(
                    round = %valid.round,
                    "We enter Propose with a valid value"
                );
            }
        }
        metrics.step_end(prev_step);
        metrics.step_start(new_step);
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
            let proposer = state.get_proposer(height, round)?;

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

            let signed_proposal = state.ctx.sign_proposal(proposal);

            perform!(
                co,
                Effect::Broadcast(SignedConsensusMsg::Proposal(signed_proposal.clone()))
            );

            on_proposal(co, state, metrics, signed_proposal).await
        }

        DriverOutput::Vote(vote) => {
            info!(
                vote_type = ?vote.vote_type(),
                id = %PrettyVal(vote.value().as_ref()),
                round = %vote.round(),
                "Voting",
            );

            let signed_vote = state.ctx.sign_vote(vote);

            perform!(
                co,
                Effect::Broadcast(SignedConsensusMsg::Vote(signed_vote.clone()))
            );

            apply_driver_input(co, state, metrics, DriverInput::Vote(signed_vote)).await
        }

        DriverOutput::Decide(consensus_round, proposal) => {
            // TODO: Remove proposal, votes, block for the round
            info!(
                round = %consensus_round,
                ?proposal,
                "Decided",
            );

            // Store value decided on for retrieval when timeout commit elapses
            state
                .decision
                .insert((state.driver.height(), consensus_round), proposal.clone());

            perform!(
                co,
                Effect::ScheduleTimeout(Timeout::commit(consensus_round))
            );

            Ok(())
        }

        DriverOutput::ScheduleTimeout(timeout) => {
            info!(round = %timeout.round, step = ?timeout.step, "Scheduling timeout");

            perform!(co, Effect::ScheduleTimeout(timeout));

            Ok(())
        }

        DriverOutput::GetValue(height, round, timeout) => {
            info!(%height, %round, "Requesting value");

            perform!(co, Effect::GetValue(height, round, timeout));

            Ok(())
        }
    }
}
