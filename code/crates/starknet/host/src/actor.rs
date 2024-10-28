#![allow(unused_variables, unused_imports)]

use std::ops::Deref;
use std::sync::Arc;

use bytes::Bytes;
use eyre::eyre;
use ractor::{async_trait, Actor, ActorProcessingErr, SpawnErr};
use rand::RngCore;
use sha3::Digest;
use tokio::time::Instant;
use tracing::{debug, error, trace};

use malachite_actors::consensus::ConsensusMsg;
use malachite_actors::gossip_consensus::{GossipConsensusMsg, GossipConsensusRef};
use malachite_actors::host::{LocallyProposedValue, ProposedValue};
use malachite_actors::util::streaming::{StreamContent, StreamId, StreamMessage};
use malachite_common::{Extension, Round, Validity};
use malachite_metrics::Metrics;
use malachite_proto::Protobuf;
use malachite_starknet_p2p_types::Transactions;

use crate::mempool::{MempoolMsg, MempoolRef};
use crate::mock::context::MockContext;
use crate::mock::host::MockHost;
use crate::part_store::PartStore;
use crate::streaming::PartStreamsMap;
use crate::types::{Address, BlockHash, Height, Proposal, ProposalPart, ValidatorSet};
use crate::Host;

pub struct StarknetHost {
    host: MockHost,
    mempool: MempoolRef,
    gossip_consensus: GossipConsensusRef<MockContext>,
    metrics: Metrics,
}

pub struct HostState {
    height: Height,
    round: Round,
    proposer: Option<Address>,
    part_store: PartStore<MockContext>,
    part_streams_map: PartStreamsMap,
    next_stream_id: StreamId,
}

impl Default for HostState {
    fn default() -> Self {
        Self {
            height: Height::new(0, 0),
            round: Round::Nil,
            proposer: None,
            part_store: PartStore::default(),
            part_streams_map: PartStreamsMap::default(),
            next_stream_id: StreamId::default(),
        }
    }
}

pub type HostRef = malachite_actors::host::HostRef<MockContext>;
pub type HostMsg = malachite_actors::host::HostMsg<MockContext>;

impl StarknetHost {
    pub fn new(
        host: MockHost,
        mempool: MempoolRef,
        gossip_consensus: GossipConsensusRef<MockContext>,
        metrics: Metrics,
    ) -> Self {
        Self {
            host,
            mempool,
            gossip_consensus,
            metrics,
        }
    }

    pub async fn spawn(
        host: MockHost,
        mempool: MempoolRef,
        gossip_consensus: GossipConsensusRef<MockContext>,
        metrics: Metrics,
    ) -> Result<HostRef, SpawnErr> {
        let (actor_ref, _) = Actor::spawn(
            None,
            Self::new(host, mempool, gossip_consensus, metrics),
            HostState::default(),
        )
        .await?;

        Ok(actor_ref)
    }

    #[tracing::instrument(skip_all, fields(%height, %round))]
    pub fn build_value_from_parts(
        &self,
        parts: &[Arc<ProposalPart>],
        height: Height,
        round: Round,
    ) -> Option<ProposedValue<MockContext>> {
        let (valid_round, value, validator_address, validity, extension) =
            self.build_proposal_content_from_parts(parts, height, round)?;

        Some(ProposedValue {
            validator_address,
            height,
            round,
            valid_round,
            value,
            validity,
            extension,
        })
    }

    #[tracing::instrument(skip_all, fields(%height, %round))]
    pub fn build_proposal_content_from_parts(
        &self,
        parts: &[Arc<ProposalPart>],
        height: Height,
        round: Round,
    ) -> Option<(Round, BlockHash, Address, Validity, Option<Extension>)> {
        if parts.is_empty() {
            return None;
        }

        let Some(init) = parts.iter().find_map(|part| part.as_init()) else {
            error!("No Init part found in the proposal parts");
            return None;
        };

        let valid_round = init.valid_round;

        let Some(fin) = parts.iter().find_map(|part| part.as_fin()) else {
            error!("No Fin part found in the proposal parts");
            return None;
        };

        trace!(parts.len = %parts.len(), "Building proposal content from parts");

        let extension = self.host.params().vote_extensions.enabled.then(|| {
            debug!(
                size = %self.host.params().vote_extensions.size,
                "Vote extensions are enabled"
            );

            let size = self.host.params().vote_extensions.size.as_u64() as usize;
            let mut bytes = vec![0u8; size];
            rand::thread_rng().fill_bytes(&mut bytes);

            Extension::from(bytes)
        });

        let block_hash = {
            let mut block_hasher = sha3::Keccak256::new();
            for part in parts {
                block_hasher.update(part.to_sign_bytes());
            }
            BlockHash::new(block_hasher.finalize().into())
        };

        trace!(%block_hash, "Computed block hash");

        // TODO: How to compute validity?
        let validity = Validity::Valid;

        Some((
            valid_round,
            block_hash,
            init.proposer.clone(),
            validity,
            extension,
        ))
    }

    #[tracing::instrument(skip_all, fields(
        part.height = %height,
        part.round = %round,
        part.message = ?part.part_type(),
    ))]
    async fn build_value_from_part(
        &self,
        state: &mut HostState,
        height: Height,
        round: Round,
        part: ProposalPart,
    ) -> Option<ProposedValue<MockContext>> {
        state.part_store.store(height, round, part.clone());

        if let ProposalPart::Transactions(txes) = &part {
            debug!("Simulating tx execution and proof verification");

            // Simulate Tx execution and proof verification (assumes success)
            // TODO: Add config knob for invalid blocks
            let num_txes = part.tx_count() as u32;
            let exec_time = self.host.params().exec_time_per_tx * num_txes;
            tokio::time::sleep(exec_time).await;

            trace!("Simulation took {exec_time:?} to execute {num_txes} txes");
        }

        let all_parts = state.part_store.all_parts(height, round);

        debug!(
            count = state.part_store.blocks_stored(),
            "The store has blocks"
        );

        // TODO: Do more validations, e.g. there is no higher tx proposal part,
        //       check that we have received the proof, etc.
        let Some(fin) = all_parts.iter().find_map(|part| part.as_fin()) else {
            debug!("Final proposal part has not been received yet");
            return None;
        };

        let block_size: usize = all_parts.iter().map(|p| p.size_bytes()).sum();
        let tx_count: usize = all_parts.iter().map(|p| p.tx_count()).sum();

        debug!(
            %tx_count, %block_size, num_parts = %all_parts.len(),
            "All parts have been received already, building value"
        );

        self.build_value_from_parts(&all_parts, height, round)
    }
}

#[async_trait]
impl Actor for StarknetHost {
    type Arguments = HostState;
    type State = HostState;
    type Msg = HostMsg;

    async fn pre_start(
        &self,
        _myself: HostRef,
        initial_state: Self::State,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(initial_state)
    }

    #[tracing::instrument("starknet.host", skip_all)]
    async fn handle(
        &self,
        _myself: HostRef,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            HostMsg::StartRound {
                height,
                round,
                proposer,
            } => {
                state.height = height;
                state.round = round;
                state.proposer = Some(proposer);

                Ok(())
            }

            HostMsg::GetValue {
                height,
                round,
                timeout_duration,
                address,
                reply_to,
            } => {
                let deadline = Instant::now() + timeout_duration;

                debug!(%height, %round, "Building new proposal...");

                let (mut rx_part, rx_hash) =
                    self.host.build_new_proposal(height, round, deadline).await;

                let stream_id = state.next_stream_id;
                state.next_stream_id += 1;

                let mut sequence = 0;
                let mut extension_part = None;

                while let Some(part) = rx_part.recv().await {
                    state.part_store.store(height, round, part.clone());

                    if let ProposalPart::Transactions(_) = &part {
                        if extension_part.is_none() {
                            extension_part = Some(part.clone());
                        }
                    }

                    debug!(
                        %stream_id,
                        %sequence,
                        "Broadcasting proposal part"
                    );

                    let msg =
                        StreamMessage::new(stream_id, sequence, StreamContent::Data(part.clone()));
                    sequence += 1;

                    self.gossip_consensus
                        .cast(GossipConsensusMsg::BroadcastProposalPart(msg))?;
                }

                let msg = StreamMessage::new(stream_id, sequence, StreamContent::Fin(true));

                self.gossip_consensus
                    .cast(GossipConsensusMsg::BroadcastProposalPart(msg))?;

                let block_hash = rx_hash.await?;
                debug!(%block_hash, "Got block");

                let parts = state.part_store.all_parts(height, round);

                let extension = extension_part
                    .and_then(|part| part.as_transactions().and_then(|txs| txs.to_bytes().ok()))
                    .map(Extension::from);

                if let Some(value) = self.build_value_from_parts(&parts, height, round) {
                    reply_to.send(LocallyProposedValue::new(
                        value.height,
                        value.round,
                        value.value,
                        extension,
                    ))?;
                }

                Ok(())
            }

            HostMsg::RestreamValue {
                height,
                round,
                valid_round,
                address,
            } => {
                let mut parts = state.part_store.all_parts(height, valid_round);
                let stream_id = state.next_stream_id;
                state.next_stream_id += 1;

                let mut extension_part = None;

                for (sequence, part) in parts.iter_mut().enumerate() {
                    let mut part = Arc::unwrap_or_clone((*part).clone());

                    // Change the Init to indicate restreaming
                    match part {
                        ProposalPart::Init(ref mut init_part) => {
                            assert_eq!(init_part.proposal_round, valid_round);
                            init_part.proposal_round = round;
                            init_part.valid_round = valid_round;
                            init_part.proposer = address.clone();
                            state.part_store.store(height, round, part.clone());
                        }
                        ProposalPart::Transactions(_) => {
                            if extension_part.is_none() {
                                extension_part = Some(part.clone());
                            }
                        }
                        _ => {}
                    }

                    debug!(
                        %stream_id,
                        %sequence,
                        "Broadcasting proposal part"
                    );

                    let msg =
                        StreamMessage::new(stream_id, sequence as u64, StreamContent::Data(part));

                    self.gossip_consensus
                        .cast(GossipConsensusMsg::BroadcastProposalPart(msg))?;
                }

                let extension = extension_part
                    .and_then(|part| part.as_transactions().and_then(|txs| txs.to_bytes().ok()))
                    .map(Extension::from);

                Ok(())
            }

            HostMsg::ReceivedProposalPart {
                from,
                part,
                reply_to,
            } => {
                let sequence = part.sequence;

                let Some(parts) = state.part_streams_map.insert(from, part) else {
                    return Ok(());
                };

                if parts.height < state.height {
                    trace!(
                        height = %state.height,
                        round = %state.round,
                        part.height = %parts.height,
                        part.round = %parts.round,
                        part.sequence = %sequence,
                        "Received outdated proposal part, ignoring"
                    );

                    return Ok(());
                }

                for part in parts.parts {
                    debug!(
                        part.sequence = %sequence,
                        part.height = %parts.height,
                        part.round = %parts.round,
                        part.message = ?part.part_type(),
                        "Processing proposal part"
                    );

                    if let Some(value) = self
                        .build_value_from_part(state, parts.height, parts.round, part)
                        .await
                    {
                        reply_to.send(value)?;
                        break;
                    }
                }

                Ok(())
            }

            HostMsg::GetValidatorSet { height, reply_to } => {
                if let Some(validators) = self.host.validators(height).await {
                    reply_to.send(ValidatorSet::new(validators))?;
                    Ok(())
                } else {
                    Err(eyre!("No validator set found for the given height {height}").into())
                }
            }

            HostMsg::Decide {
                height,
                round,
                value: block_hash,
                commits,
                consensus,
            } => {
                let all_parts = state.part_store.all_parts(height, round);

                // TODO: Build the block from proposal parts and commits and store it

                // Update metrics
                let block_size: usize = all_parts.iter().map(|p| p.size_bytes()).sum();
                let extension_size: usize = commits
                    .iter()
                    .map(|c| c.extension.as_ref().map(|e| e.size_bytes()).unwrap_or(0))
                    .sum();

                let block_and_commits_size = block_size + extension_size;
                let tx_count: usize = all_parts.iter().map(|p| p.tx_count()).sum();

                self.metrics.block_tx_count.observe(tx_count as f64);
                self.metrics
                    .block_size_bytes
                    .observe(block_and_commits_size as f64);
                self.metrics.finalized_txes.inc_by(tx_count as u64);

                // Send Update to mempool to remove all the tx-es included in the block.
                let mut tx_hashes = vec![];
                for part in all_parts {
                    if let ProposalPart::Transactions(txes) = &part.as_ref() {
                        tx_hashes.extend(txes.as_slice().iter().map(|tx| tx.hash()));
                    }
                }

                // Prune the PartStore of all parts for heights lower than `state.height`
                state.part_store.prune(state.height);

                // Notify the mempool to remove corresponding txs
                self.mempool.cast(MempoolMsg::Update { tx_hashes })?;

                // Notify Starknet Host of the decision
                self.host.decision(block_hash, commits, height).await;

                // Start the next height
                consensus.cast(ConsensusMsg::StartHeight(state.height.increment()))?;

                Ok(())
            }
        }
    }
}
