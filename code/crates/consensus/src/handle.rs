use async_recursion::async_recursion;
use tracing::{debug, error, info, trace, warn};

use malachite_common::*;
use malachite_driver::Input as DriverInput;
use malachite_driver::Output as DriverOutput;
use malachite_metrics::Metrics;

use crate::effect::{Effect, Resume};
use crate::error::Error;
use crate::gen::Co;
use crate::msg::Msg;
use crate::perform;
use crate::state::State;
use crate::types::{Block, GossipEvent, GossipMsg, PeerId, SignedMessage};
use crate::util::pretty::{PrettyProposal, PrettyVal, PrettyVote};

pub async fn handle<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    msg: Msg<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    handle_msg(&co, state, metrics, msg).await
}

#[async_recursion]
async fn handle_msg<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    msg: Msg<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    match msg {
        Msg::StartHeight(height) => start_height(co, state, metrics, height).await,
        Msg::GossipEvent(event) => on_gossip_event(co, state, metrics, event).await,
        Msg::ProposeValue(height, round, value) => {
            propose_value(co, state, metrics, height, round, value).await
        }
        Msg::TimeoutElapsed(timeout) => on_timeout_elapsed(co, state, metrics, timeout).await,
        Msg::BlockReceived(block) => on_received_block(co, state, metrics, block).await,
    }
}

async fn start_height<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    height: Ctx::Height,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let round = Round::new(0);

    let proposer = state.get_proposer(height, round).cloned()?;
    info!(%proposer, "Proposer found for next height.");

    apply_driver_input(
        co,
        state,
        metrics,
        DriverInput::NewRound(height, round, proposer),
    )
    .await?;

    metrics.block_start();
    metrics.height.set(height.as_u64() as i64);
    metrics.round.set(round.as_i64());

    replay_pending_msgs(co, state, metrics).await?;

    Ok(())
}

async fn move_to_height<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    height: Ctx::Height,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    perform!(co, Effect::CancelAllTimeouts);
    perform!(co, Effect::ResetTimeouts);

    // End the current step (most likely Commit)
    metrics.step_end(state.driver.step());

    let validator_set = perform!(co, Effect::GetValidatorSet(height),
        Resume::ValidatorSet(vs_height, validator_set) => {
            if vs_height == height {
                Ok(validator_set)
            } else {
                Err(Error::UnexpectedResume(
                    Resume::ValidatorSet(vs_height, validator_set),
                    "ValidatorSet for the current height"
                ))
            }
        }
    )?;

    state.driver.move_to_height(height, validator_set);

    debug_assert_eq!(state.driver.height(), height);
    debug_assert_eq!(state.driver.round(), Round::Nil);

    start_height(co, state, metrics, height).await
}

async fn replay_pending_msgs<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let pending_msgs = std::mem::take(&mut state.msg_queue);
    info!(
        msg_count = pending_msgs.len(),
        "Replay messages for next height."
    );

    for pending_msg in pending_msgs {
        handle_msg(co, state, metrics, pending_msg).await?;
    }

    Ok(())
}

#[async_recursion]
async fn apply_driver_input<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    input: DriverInput<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    match &input {
        DriverInput::NewRound(_, _, _) => {
            perform!(co, Effect::CancelAllTimeouts);
        }

        DriverInput::ProposeValue(round, _) => {
            perform!(co, Effect::CancelTimeout(Timeout::propose(*round)));
        }

        DriverInput::Proposal(proposal, _) => {
            if proposal.height() != state.driver.height() {
                warn!(
                    proposal_height=%proposal.height(),
                    "Ignore proposal for different height."
                );

                return Ok(());
            }

            if proposal.round() != state.driver.round() {
                warn!(
                    proposal_round=%proposal.round(),
                    "Ignore proposal for different round."
                );

                return Ok(());
            }

            perform!(
                co,
                Effect::CancelTimeout(Timeout::propose(proposal.round()))
            );
        }

        DriverInput::Vote(vote) => {
            if vote.height() != state.driver.height() {
                warn!(
                    vote_height=%vote.height(),
                    "Ignore vote for different height."
                );

                return Ok(());
            }

            if vote.round() != state.driver.round() {
                warn!(
                    vote_round=%vote.round(),
                    "Ignore vote for different round."
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
        debug!(?prev_step, ?new_step, "Change step.");

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
            metrics.round.set(round.as_i64());

            let proposer = state.get_proposer(height, round)?;
            info!(%proposer, "Proposer found for next round.");

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
                "Propose value."
            );

            let signed_proposal = state.ctx.sign_proposal(proposal);

            perform!(
                co,
                Effect::Broadcast(GossipMsg::Proposal(signed_proposal.clone()))
            );

            apply_driver_input(
                co,
                state,
                metrics,
                DriverInput::Proposal(signed_proposal.proposal, Validity::Valid),
            )
            .await
        }

        DriverOutput::Vote(vote) => {
            info!(
                vote=?vote.vote_type(),
                value=%PrettyVal(vote.value().as_ref()),
                "Vote value.",
            );

            let signed_vote = state.ctx.sign_vote(vote);

            perform!(co, Effect::Broadcast(GossipMsg::Vote(signed_vote.clone()),));

            apply_driver_input(co, state, metrics, DriverInput::Vote(signed_vote.vote)).await
        }

        DriverOutput::Decide(round, value) => {
            // TODO: Remove proposal, votes, block for the round
            info!(value=%value.id(), "Decide value.");

            perform!(co, Effect::ScheduleTimeout(Timeout::commit(round)));

            decided(co, state, metrics, state.driver.height(), round, value).await
        }

        DriverOutput::ScheduleTimeout(timeout) => {
            debug!(%timeout, "Schedule timeout.");

            perform!(co, Effect::ScheduleTimeout(timeout));

            Ok(())
        }

        DriverOutput::GetValue(height, round, timeout) => {
            trace!(%timeout, "Request value.");

            perform!(co, Effect::GetValue(height, round, timeout));

            Ok(())
        }
    }
}

async fn propose_value<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    height: Ctx::Height,
    round: Round,
    value: Ctx::Value,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    if state.driver.height() != height {
        warn!(
            %height,
            "Ignore proposal for different height."
        );

        return Ok(());
    }

    if state.driver.round() != round {
        warn!(
            %round,
            "Ignore proposal for different round."
        );

        return Ok(());
    }

    apply_driver_input(co, state, metrics, DriverInput::ProposeValue(round, value)).await
}

async fn decided<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    height: Ctx::Height,
    round: Round,
    value: Ctx::Value,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    // Remove the block information as it is not needed anymore
    state.remove_received_block(height, round);

    // Restore the commits. Note that they will be removed from `state`
    let commits = state.restore_precommits(height, round, &value);

    perform!(
        co,
        Effect::DecidedOnValue {
            height,
            round,
            value,
            commits
        }
    );

    // Reinitialize to remove any previous round or equivocating precommits.
    // TODO: Revise when evidence module is added.
    state.signed_precommits.clear();

    metrics.block_end();
    metrics.finalized_blocks.inc();
    metrics
        .rounds_per_block
        .observe((round.as_i64() + 1) as f64);

    Ok(())
}

async fn on_gossip_event<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    event: GossipEvent<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    match event {
        GossipEvent::Listening(_) => Ok(()),

        GossipEvent::PeerConnected(peer_id) => {
            if !state.connected_peers.insert(peer_id) {
                // We already saw that peer, ignoring...
                return Ok(());
            }

            info!(%peer_id, "Connect to peer.");

            metrics.connected_peers.inc();

            if state.connected_peers.len() == state.driver.validator_set.count() - 1 {
                info!(
                    peers_count = state.connected_peers.len(),
                    "Start consensus."
                );

                start_height(co, state, metrics, state.driver.height()).await?;
            }

            Ok(())
        }

        GossipEvent::PeerDisconnected(peer_id) => {
            info!(%peer_id, "Disconnect from peer.");

            if state.connected_peers.remove(&peer_id) {
                metrics.connected_peers.dec();

                // TODO: pause/stop consensus, if necessary
            }

            Ok(())
        }

        GossipEvent::Message(from, msg) => {
            let Some(msg_height) = msg.msg_height() else {
                error!("Drop received message without height.");
                return Ok(());
            };

            // Queue messages if driver is not initialized, or if they are for higher height.
            // Process messages received for the current height.
            // Drop all others.
            if state.driver.round() == Round::Nil {
                debug!("Receive gossip event at round -1, queue for later");

                state
                    .msg_queue
                    .push_back(Msg::GossipEvent(GossipEvent::Message(from, msg)));
            } else if state.driver.height() < msg_height {
                debug!("Receive gossip event for higher height, queue for later");

                state
                    .msg_queue
                    .push_back(Msg::GossipEvent(GossipEvent::Message(from, msg)));
            } else if state.driver.height() == msg_height {
                on_gossip_msg(co, state, metrics, from, msg).await?;
            }

            Ok(())
        }
    }
}

async fn on_gossip_msg<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    from: PeerId,
    msg: GossipMsg<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    match msg {
        GossipMsg::Vote(signed_vote) => {
            let validator_address = signed_vote.validator_address();

            debug!(peer_id = %from, %validator_address, vote=%PrettyVote::<Ctx>(&signed_vote.vote), "Receive vote.");

            if signed_vote.vote.height() != state.driver.height() {
                warn!(
                    peer_id=%from, validator = %validator_address,
                    signed_vote_height=%signed_vote.vote.height(),
                    "Ignore signed vote for different height."
                );

                return Ok(());
            }

            let Some(validator) = state.driver.validator_set.get_by_address(validator_address)
            else {
                error!(
                    peer_id = %from, %validator_address,
                    "Receive vote from unknown validator."
                );

                return Ok(());
            };

            let signed_msg = SignedMessage::Vote(signed_vote.clone());
            let verify_sig = Effect::VerifySignature(signed_msg, validator.public_key().clone());
            if !perform!(co, verify_sig, Resume::SignatureValidity(valid) => valid) {
                error!(
                    peer_id=%from, %validator_address, vote=%PrettyVote::<Ctx>(&signed_vote.vote),
                    "Receive invalid vote."
                );

                return Ok(());
            }

            // Store the non-nil Precommits.
            if signed_vote.vote.vote_type() == VoteType::Precommit
                && signed_vote.vote.value().is_val()
            {
                state.store_signed_precommit(signed_vote.clone());
            }

            apply_driver_input(co, state, metrics, DriverInput::Vote(signed_vote.vote)).await?;
        }

        GossipMsg::Proposal(signed_proposal) => {
            let validator = signed_proposal.proposal.validator_address();

            info!(peer_id=%from, validator_address=%validator, proposal = %PrettyProposal::<Ctx>(&signed_proposal.proposal), "Receive proposal.");

            let Some(validator) = state.driver.validator_set.get_by_address(validator) else {
                error!(peer_id=%from, validator_address=%validator, "Receive proposal from unknown validator.");
                return Ok(());
            };

            let proposal = &signed_proposal.proposal;
            let proposal_height = proposal.height();
            let proposal_round = proposal.round();

            if proposal_height != state.driver.height() {
                warn!(
                    %proposal_height,
                    "Ignore proposal for different height."
                );

                return Ok(());
            }

            if proposal_round != state.driver.round() {
                warn!(
                    %proposal_round,
                    "Ignore proposal for different round."
                );

                return Ok(());
            }

            let signed_msg = SignedMessage::Proposal(signed_proposal.clone());
            let verify_sig = Effect::VerifySignature(signed_msg, validator.public_key().clone());
            if !perform!(co, verify_sig, Resume::SignatureValidity(valid) => valid) {
                error!(
                    proposal = %PrettyProposal::<Ctx>(&signed_proposal.proposal),
                    "Receive invalid signature."
                );

                return Ok(());
            }

            let received_block = state
                .received_blocks
                .iter()
                .find(|(height, round, ..)| height == &proposal_height && round == &proposal_round);

            match received_block {
                Some((_height, _round, _value, valid)) => {
                    apply_driver_input(
                        co,
                        state,
                        metrics,
                        DriverInput::Proposal(proposal.clone(), *valid),
                    )
                    .await?;
                }
                None => {
                    // Store the proposal and wait for all block parts
                    // TODO: or maybe integrate with receive-proposal() here? will this block until all parts are received?

                    info!(
                        height = %proposal.height(),
                        round = %proposal.round(),
                        "Store proposal received before all block parts."
                    );

                    // FIXME: Avoid mutating the driver state directly
                    state.driver.proposal = Some(proposal.clone());
                }
            }
        }

        GossipMsg::BlockPart(signed_block_part) => {
            let validator_address = signed_block_part.validator_address();

            let Some(validator) = state.driver.validator_set.get_by_address(validator_address)
            else {
                error!(peer_id=%from, %validator_address, "Receive block part from unknown validator.");
                return Ok(());
            };

            let signed_msg = SignedMessage::BlockPart(signed_block_part.clone());
            let verify_sig = Effect::VerifySignature(signed_msg, validator.public_key().clone());
            if !perform!(co, verify_sig, Resume::SignatureValidity(valid) => valid) {
                error!(peer_id=%from, %validator_address, ?signed_block_part, "Receive invalid block part.");
                return Ok(());
            }

            // TODO: Verify that the proposal was signed by the proposer for the height and round, drop otherwise.
            perform!(co, Effect::ReceivedBlockPart(signed_block_part.block_part));
        }
    }

    Ok(())
}

async fn on_timeout_elapsed<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    timeout: Timeout,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let height = state.driver.height();
    let round = state.driver.round();

    if timeout.round != round {
        debug!(
            timeout_round = %timeout.round,
            "Ignore timeout for different round.",

        );

        return Ok(());
    }

    info!(%timeout, "Elapse timeout.");

    apply_driver_input(co, state, metrics, DriverInput::TimeoutElapsed(timeout)).await?;

    if timeout.step == TimeoutStep::Commit {
        move_to_height(co, state, metrics, height.increment()).await?;
    }

    Ok(())
}

async fn on_received_block<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    block: Block<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let Block {
        height,
        round,
        value,
        validity,
        ..
    } = block;

    info!(%height, %round, value=%value.id(), "Receive block.");

    // Store the block and validity information. It will be removed when a decision is reached for that height.
    state
        .received_blocks
        .push((height, round, value.clone(), validity));

    if let Some(proposal) = state.driver.proposal.as_ref() {
        if height != proposal.height() || round != proposal.round() {
            // The block we received is not for the current proposal, ignoring
            return Ok(());
        }

        let validity = Validity::from_bool(proposal.value() == &value && validity.is_valid());

        apply_driver_input(
            co,
            state,
            metrics,
            DriverInput::Proposal(proposal.clone(), validity),
        )
        .await?;
    }

    Ok(())
}
