use std::time::Duration;

use eyre::eyre;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{error, info};

use malachitebft_app_channel::app::engine::host::Next;
use malachitebft_app_channel::app::streaming::StreamContent;
use malachitebft_app_channel::app::types::core::{Height as _, Round, Validity};
use malachitebft_app_channel::app::types::sync::RawDecidedValue;
use malachitebft_app_channel::app::types::{LocallyProposedValue, ProposedValue};
use malachitebft_app_channel::{
    AppMsg, Channels, ConsensusRequest, ConsensusRequestError, NetworkMsg,
};
use malachitebft_test::{Height, TestContext};

use crate::state::{decode_value, encode_value, State};

/// Periodically request a state dump from consensus and print it to the console
fn monitor_state(tx_request: mpsc::Sender<ConsensusRequest<TestContext>>) {
    tokio::spawn(async move {
        loop {
            match ConsensusRequest::dump_state(&tx_request).await {
                Ok(dump) => {
                    tracing::debug!("State dump: {dump:#?}");
                }
                Err(ConsensusRequestError::Recv) => {
                    tracing::error!("Failed to receive state dump from consensus");
                }
                Err(ConsensusRequestError::Full) => {
                    tracing::error!("Consensus request channel full");
                }
                Err(ConsensusRequestError::Closed) => {
                    tracing::error!("Consensus request channel closed");
                }
            }

            sleep(Duration::from_secs(1)).await;
        }
    });
}

pub async fn run(state: &mut State, channels: &mut Channels<TestContext>) -> eyre::Result<()> {
    // If the MALACHITE_MONITOR_STATE env var is set, start monitoring the consensus state
    if std::env::var("MALACHITE_MONITOR_STATE").is_ok() {
        monitor_state(channels.requests.clone());
    }

    while let Some(msg) = channels.consensus.recv().await {
        match msg {
            // The first message to handle is the `ConsensusReady` message, signaling to the app
            // that Malachite is ready to start consensus
            AppMsg::ConsensusReady { reply, .. } => {
                let start_height = state
                    .store
                    .max_decided_value_height()
                    .await
                    .map(|height| height.increment())
                    .unwrap_or_else(|| Height::INITIAL);

                info!(%start_height, "Consensus is ready");

                sleep(Duration::from_millis(200)).await;

                if reply
                    .send((start_height, state.get_validator_set(start_height).clone()))
                    .is_err()
                {
                    error!("Failed to send ConsensusReady reply");
                }
            }

            // The next message to handle is the `StartRound` message, signaling to the app
            // that consensus has entered a new round (including the initial round 0)
            AppMsg::StartedRound {
                height,
                round,
                proposer,
                role,
                reply_value,
            } => {
                info!(%height, %round, %proposer, ?role, "Started round");

                reload_log_level(height, round);

                // We can use that opportunity to update our internal state
                state.current_height = height;
                state.current_round = round;
                state.current_proposer = Some(proposer);

                let pending_parts = state
                    .store
                    .get_pending_proposal_parts(height, round)
                    .await?;
                info!(%height, %round, "Found {} pending proposal parts, validating...", pending_parts.len());

                for parts in &pending_parts {
                    // Remove the parts from pending
                    state
                        .store
                        .remove_pending_proposal_parts(parts.clone())
                        .await?;

                    match state.validate_proposal_parts(parts) {
                        Ok(()) => {
                            // Validation passed - convert to ProposedValue and move to undecided
                            let value = State::assemble_value_from_parts(parts.clone())?;
                            state.store.store_undecided_proposal(value).await?;
                            info!(
                                height = %parts.height,
                                round = %parts.round,
                                proposer = %parts.proposer,
                                "Moved valid pending proposal to undecided after validation"
                            );
                        }
                        Err(error) => {
                            // Validation failed, log error
                            error!(
                                height = %parts.height,
                                round = %parts.round,
                                proposer = %parts.proposer,
                                error = ?error,
                                "Removed invalid pending proposal"
                            );
                        }
                    }
                }

                // If we have already built or seen values for this height and round,
                // send them all back to consensus. This may happen when we are restarting after a crash.
                let proposals = state.store.get_undecided_proposals(height, round).await?;
                info!(%height, %round, "Found {} undecided proposals", proposals.len());

                if reply_value.send(proposals).is_err() {
                    error!("Failed to send undecided proposals");
                }
            }

            // At some point, we may end up being the proposer for that round, and the engine
            // will then ask us for a value to propose to the other validators.
            AppMsg::GetValue {
                height,
                round,
                timeout: _,
                reply,
            } => {
                // NOTE: We can ignore the timeout as we are building the value right away.
                // If we were let's say reaping as many txes from a mempool and executing them,
                // then we would need to respect the timeout and stop at a certain point.

                info!(%height, %round, "Consensus is requesting a value to propose");

                // Here it is important that, if we have previously built a value for this height and round,
                // we send back the very same value.
                let proposal = match state.get_previously_built_value(height, round).await? {
                    Some(proposal) => {
                        info!(value = %proposal.value.id(), "Re-using previously built value");
                        proposal
                    }
                    None => {
                        // If we have not previously built a value for that very same height and round,
                        // we need to create a new value to propose and send it back to consensus.
                        info!("Building a new value to propose");
                        state.propose_value(height, round).await?
                    }
                };

                // Send it to consensus
                if reply.send(proposal.clone()).is_err() {
                    error!("Failed to send GetValue reply");
                }

                // The POL round is always nil when we propose a newly built value.
                // See L15/L18 of the Tendermint algorithm.
                let pol_round = Round::Nil;

                // Now what's left to do is to break down the value to propose into parts,
                // and send those parts over the network to our peers, for them to re-assemble the full value.
                for stream_message in state.stream_proposal(proposal, pol_round) {
                    info!(%height, %round, "Streaming proposal part: {stream_message:?}");

                    channels
                        .network
                        .send(NetworkMsg::PublishProposalPart(stream_message))
                        .await?;
                }
            }

            AppMsg::ExtendVote {
                height: _,
                round: _,
                value_id: _,
                reply,
            } => {
                // TODO
                if reply.send(None).is_err() {
                    error!("Failed to send ExtendVote reply");
                }
            }

            AppMsg::VerifyVoteExtension {
                height: _,
                round: _,
                value_id: _,
                extension: _,
                reply,
            } => {
                // TODO
                if reply.send(Ok(())).is_err() {
                    error!("Failed to send VerifyVoteExtension reply");
                }
            }

            // On the receiving end of these proposal parts (ie. when we are not the proposer),
            // we need to process these parts and re-assemble the full value.
            // To this end, we store each part that we receive and assemble the full value once we
            // have all its constituent parts. Then we send that value back to consensus for it to
            // consider and vote for or against it (ie. vote `nil`), depending on its validity.
            AppMsg::ReceivedProposalPart { from, part, reply } => {
                let part_type = match &part.content {
                    StreamContent::Data(part) => part.get_type(),
                    StreamContent::Fin => "end of stream",
                };

                info!(%from, %part.sequence, part.type = %part_type, "Received proposal part");

                let proposed_value = state.received_proposal_part(from, part).await?;

                if reply.send(proposed_value).is_err() {
                    error!("Failed to send ReceivedProposalPart reply");
                }
            }

            // After some time, consensus will finally reach a decision on the value
            // to commit for the current height, and will notify the application,
            // providing it with a commit certificate which contains the ID of the value
            // that was decided on as well as the set of commits for that value,
            // ie. the precommits together with their (aggregated) signatures.
            AppMsg::Decided {
                certificate,
                extensions,
                reply,
            } => {
                info!(
                    height = %certificate.height,
                    round = %certificate.round,
                    value = %certificate.value_id,
                    "Consensus has decided on value, committing..."
                );

                // When that happens, we store the decided value in our store
                match state.commit(certificate, extensions).await {
                    Ok(_) => {
                        // Sleep a bit to slow down the app.
                        sleep(Duration::from_millis(500)).await;

                        // And then we instruct consensus to start the next height
                        if reply
                            .send(Next::Start(
                                state.current_height,
                                state.get_validator_set(state.current_height).clone(),
                            ))
                            .is_err()
                        {
                            error!("Failed to send StartHeight reply");
                        }
                    }
                    Err(_) => {
                        let height = state.current_height;

                        // Commit failed, restart the height
                        error!("Commit failed, restarting height {height}");

                        if reply
                            .send(Next::Restart(
                                height,
                                state.get_validator_set(height).clone(),
                            ))
                            .is_err()
                        {
                            error!("Failed to send RestartHeight reply");
                        }
                    }
                }
            }

            // It may happen that our node is lagging behind its peers. In that case,
            // a synchronization mechanism will automatically kick to try and catch up to
            // our peers. When that happens, some of these peers will send us decided values
            // for the current height only (not for future heights). When the engine receives
            // such a value, it will forward to the application to decode it from its wire format
            // and send back the decoded value to consensus.
            AppMsg::ProcessSyncedValue {
                height,
                round,
                proposer,
                value_bytes,
                reply,
            } => {
                info!(%height, %round, "Processing synced value");

                if let Some(value) = decode_value(value_bytes) {
                    let proposed_value = ProposedValue {
                        height,
                        round,
                        valid_round: Round::Nil,
                        proposer,
                        value,
                        validity: Validity::Valid,
                    };

                    // TODO: We plan to add some validation here in the future.
                    state
                        .store
                        .store_undecided_proposal(proposed_value.clone())
                        .await?;

                    if reply.send(Some(proposed_value)).is_err() {
                        error!("Failed to send ProcessSyncedValue reply");
                    }
                } else if reply.send(None).is_err() {
                    error!("Failed to send ProcessSyncedValue reply");
                }
            }

            // If, on the other hand, we are not lagging behind but are instead asked by one of
            // our peer to help them catch up because they are the one lagging behind,
            // then the engine might ask the application to provide with the value
            // that was decided at some lower height. In that case, we fetch it from our store
            // and send it to consensus.
            AppMsg::GetDecidedValue { height, reply } => {
                info!(%height, "Received sync request for decided value");

                let decided_value = state.get_decided_value(height).await;
                info!(%height, "Found decided value: {decided_value:?}");

                let raw_decided_value = decided_value.map(|decided_value| RawDecidedValue {
                    certificate: decided_value.certificate,
                    value_bytes: encode_value(&decided_value.value),
                });

                if reply.send(raw_decided_value).is_err() {
                    error!("Failed to send GetDecidedValue reply");
                }
            }

            // In order to figure out if we can help a peer that is lagging behind,
            // the engine may ask us for the height of the earliest available value in our store.
            AppMsg::GetHistoryMinHeight { reply } => {
                let min_height = state.get_earliest_height().await;

                if reply.send(min_height).is_err() {
                    error!("Failed to send GetHistoryMinHeight reply");
                }
            }

            AppMsg::RestreamProposal {
                height,
                round,
                valid_round,
                address: _,
                value_id,
            } => {
                //  Look for a proposal at valid_round or round(should be already stored)
                let proposal_round = if valid_round == Round::Nil {
                    round
                } else {
                    valid_round
                };
                info!(%height, %proposal_round, "Restreaming existing propos*al...");

                let proposal = state
                    .store
                    .get_undecided_proposal(height, proposal_round, value_id)
                    .await?;

                if let Some(proposal) = proposal {
                    let locally_proposed_value = LocallyProposedValue {
                        height,
                        round,
                        value: proposal.value,
                    };

                    for stream_message in state.stream_proposal(locally_proposed_value, valid_round)
                    {
                        info!(%height, %valid_round, "Publishing proposal part: {stream_message:?}");

                        channels
                            .network
                            .send(NetworkMsg::PublishProposalPart(stream_message))
                            .await?;
                    }
                }
            }
        }
    }

    // If we get there, it can only be because the channel we use to receive message
    // from consensus has been closed, meaning that the consensus actor has died.
    // We can do nothing but return an error here.
    Err(eyre!("Consensus channel closed unexpectedly"))
}

/// Reload the tracing subscriber log level based on the current height and round.
/// This is useful to increase the log level when debugging a specific height and round.
///
/// If the round is greater than 0, we increase the log level to `Debug`.
/// If we are back to round 0, we reset the log level to the default one.
fn reload_log_level(_height: Height, round: Round) {
    use malachitebft_test_cli::logging;

    if round.as_i64() > 0 {
        logging::reload(logging::LogLevel::Debug);
    } else {
        logging::reset();
    }
}
