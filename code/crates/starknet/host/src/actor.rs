use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::eyre;

use itertools::Itertools;
use ractor::{async_trait, Actor, ActorProcessingErr, SpawnErr};
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use sha3::Digest;
use tokio::time::Instant;
use tracing::{debug, error, trace, warn};

use malachite_actors::consensus::ConsensusMsg;
use malachite_actors::gossip_consensus::{GossipConsensusMsg, GossipConsensusRef};
use malachite_actors::host::{LocallyProposedValue, ProposedValue};
use malachite_actors::util::streaming::{StreamContent, StreamId, StreamMessage};
use malachite_blocksync::SyncedBlock;
use malachite_common::{Round, SignedExtension, Validity};
use malachite_metrics::Metrics;
use malachite_starknet_p2p_types::{Block, PartType};

use crate::block_store::BlockStore;
use crate::mempool::{MempoolMsg, MempoolRef};
use crate::mock::host::{compute_proposal_hash, compute_proposal_signature, MockHost};
use crate::proto::Protobuf;
use crate::streaming::PartStreamsMap;
use crate::types::*;
use crate::Host;

pub struct StarknetHost {
    mempool: MempoolRef,
    gossip_consensus: GossipConsensusRef<MockContext>,
    metrics: Metrics,
}

pub struct HostState {
    height: Height,
    round: Round,
    proposer: Option<Address>,
    host: MockHost,
    block_store: BlockStore,
    part_streams_map: PartStreamsMap,
    next_stream_id: StreamId,
}

impl HostState {
    pub fn new<R>(host: MockHost, db_path: impl AsRef<Path>, rng: &mut R) -> Self
    where
        R: RngCore,
    {
        Self {
            height: Height::new(0, 0),
            round: Round::Nil,
            proposer: None,
            host,
            block_store: BlockStore::new(db_path).unwrap(),
            part_streams_map: PartStreamsMap::default(),
            next_stream_id: rng.next_u64(),
        }
    }

    pub fn next_stream_id(&mut self) -> StreamId {
        let stream_id = self.next_stream_id;
        // Wrap around if we get to u64::MAX, which may happen if the initial
        // stream id was close to it already.
        self.next_stream_id = self.next_stream_id.wrapping_add(1);
        stream_id
    }

    #[tracing::instrument(skip_all, fields(%height, %round))]
    pub async fn build_block_from_parts(
        &self,
        parts: &[Arc<ProposalPart>],
        height: Height,
        round: Round,
    ) -> Option<(ProposedValue<MockContext>, Block)> {
        let value = self.build_value_from_parts(parts, height, round).await?;

        let txes = parts
            .iter()
            .filter_map(|part| part.as_transactions())
            .flat_map(|txes| txes.to_vec())
            .collect::<Vec<_>>();

        let block = Block {
            height,
            transactions: Transactions::new(txes),
            block_hash: value.value,
        };

        Some((value, block))
    }

    #[tracing::instrument(skip_all, fields(%height, %round))]
    pub async fn build_value_from_parts(
        &self,
        parts: &[Arc<ProposalPart>],
        height: Height,
        round: Round,
    ) -> Option<ProposedValue<MockContext>> {
        let (valid_round, value, validator_address, validity, extension) = self
            .build_proposal_content_from_parts(parts, height, round)
            .await?;

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

    #[allow(clippy::type_complexity)]
    #[tracing::instrument(skip_all, fields(%height, %round))]
    pub async fn build_proposal_content_from_parts(
        &self,
        parts: &[Arc<ProposalPart>],
        height: Height,
        round: Round,
    ) -> Option<(
        Round,
        BlockHash,
        Address,
        Validity,
        Option<SignedExtension<MockContext>>,
    )> {
        if parts.is_empty() {
            return None;
        }

        let Some(init) = parts.iter().find_map(|part| part.as_init()) else {
            error!("No Init part found in the proposal parts");
            return None;
        };

        let valid_round = init.valid_round;
        if valid_round.is_defined() {
            debug!("Reassembling a Proposal we might have seen before: {init:?}");
        }

        let Some(fin) = parts.iter().find_map(|part| part.as_fin()) else {
            error!("No Fin part found in the proposal parts");
            return None;
        };

        trace!(parts.len = %parts.len(), "Building proposal content from parts");

        let extension = self.host.generate_vote_extension(height, round);

        let block_hash = {
            let mut block_hasher = sha3::Keccak256::new();
            for part in parts {
                if part.as_init().is_some() || part.as_fin().is_some() {
                    // NOTE: We do not hash over Init, so restreaming returns the same hash
                    // NOTE: We do not hash over Fin, because Fin includes a signature over the block hash
                    // TODO: we should probably still include height
                    continue;
                }

                block_hasher.update(part.to_sign_bytes());
            }

            BlockHash::new(block_hasher.finalize().into())
        };

        trace!(%block_hash, "Computed block hash");

        let proposal_hash = compute_proposal_hash(init, &block_hash);

        let validity = self
            .verify_proposal_validity(init, &proposal_hash, &fin.signature)
            .await?;

        Some((
            valid_round,
            block_hash,
            init.proposer.clone(),
            validity,
            extension,
        ))
    }

    async fn verify_proposal_validity(
        &self,
        init: &ProposalInit,
        proposal_hash: &Hash,
        signature: &Signature,
    ) -> Option<Validity> {
        let validators = self.host.validators(init.height).await?;

        let public_key = validators
            .iter()
            .find(|v| v.address == init.proposer)
            .map(|v| v.public_key);

        let Some(public_key) = public_key else {
            error!(proposer = %init.proposer, "No validator found for the proposer");
            return None;
        };

        let valid = public_key.verify(&proposal_hash.as_felt(), signature);
        Some(Validity::from_bool(valid))
    }

    #[tracing::instrument(skip_all, fields(
        part.height = %height,
        part.round = %round,
        part.message = ?part.part_type(),
    ))]
    async fn build_value_from_part(
        &mut self,
        height: Height,
        round: Round,
        part: ProposalPart,
    ) -> Option<ProposedValue<MockContext>> {
        self.host.part_store.store(height, round, part.clone());

        if let ProposalPart::Transactions(_txes) = &part {
            debug!("Simulating tx execution and proof verification");

            // Simulate Tx execution and proof verification (assumes success)
            // TODO: Add config knob for invalid blocks
            let num_txes = part.tx_count() as u32;
            let exec_time = self.host.params.exec_time_per_tx * num_txes;
            tokio::time::sleep(exec_time).await;

            trace!("Simulation took {exec_time:?} to execute {num_txes} txes");
        }

        let parts = self.host.part_store.all_parts(height, round);

        trace!(
            count = self.host.part_store.blocks_count(),
            "Blocks for which we have parts"
        );

        // TODO: Do more validations, e.g. there is no higher tx proposal part,
        //       check that we have received the proof, etc.
        let Some(_fin) = parts.iter().find_map(|part| part.as_fin()) else {
            debug!("Final proposal part has not been received yet");
            return None;
        };

        let block_size: usize = parts.iter().map(|p| p.size_bytes()).sum();
        let tx_count: usize = parts.iter().map(|p| p.tx_count()).sum();

        debug!(
            tx.count = %tx_count, block.size = %block_size, parts.count = %parts.len(),
            "All parts have been received already, building value"
        );

        let result = self.build_value_from_parts(&parts, height, round).await;

        if let Some(ref proposed_value) = result {
            self.host
                .part_store
                .store_value_id(height, round, proposed_value.value);
        }

        result
    }
}

pub type HostRef = malachite_actors::host::HostRef<MockContext>;
pub type HostMsg = malachite_actors::host::HostMsg<MockContext>;

impl StarknetHost {
    pub async fn spawn(
        home_dir: PathBuf,
        host: MockHost,
        mempool: MempoolRef,
        gossip_consensus: GossipConsensusRef<MockContext>,
        metrics: Metrics,
    ) -> Result<HostRef, SpawnErr> {
        let db_dir = home_dir.join("db");
        std::fs::create_dir_all(&db_dir).map_err(|e| SpawnErr::StartupFailed(e.into()))?;
        let db_path = db_dir.join("blocks.db");

        let (actor_ref, _) = Actor::spawn(
            None,
            Self::new(mempool, gossip_consensus, metrics),
            HostState::new(host, db_path, &mut StdRng::from_entropy()),
        )
        .await?;

        Ok(actor_ref)
    }

    pub fn new(
        mempool: MempoolRef,
        gossip_consensus: GossipConsensusRef<MockContext>,
        metrics: Metrics,
    ) -> Self {
        Self {
            mempool,
            gossip_consensus,
            metrics,
        }
    }

    async fn prune_block_store(&self, state: &mut HostState) {
        let max_height = state.block_store.last_height().unwrap_or_default();
        let max_retain_blocks = state.host.params.max_retain_blocks as u64;

        // Compute the height to retain blocks higher than
        let retain_height = max_height.as_u64().saturating_sub(max_retain_blocks);
        if retain_height <= 1 {
            // No need to prune anything, since we would retain every blocks
            return;
        }

        let retain_height = Height::new(retain_height, max_height.fork_id);
        match state.block_store.prune(retain_height).await {
            Ok(pruned) => {
                debug!(
                    %retain_height, pruned_heights = pruned.iter().join(", "),
                    "Pruned the block store"
                );
            }
            Err(e) => {
                error!(%e, %retain_height, "Failed to prune the block store");
            }
        }
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

    async fn handle(
        &self,
        _myself: HostRef,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            HostMsg::ConsensusReady(consensus) => {
                let latest_block_height = state.block_store.last_height().unwrap_or_default();
                let start_height = latest_block_height.increment();

                consensus.cast(ConsensusMsg::StartHeight(
                    start_height,
                    state.host.validator_set.clone(),
                ))?;

                Ok(())
            }

            HostMsg::StartedRound {
                height,
                round,
                proposer,
            } => {
                state.height = height;
                state.round = round;
                state.proposer = Some(proposer);

                Ok(())
            }

            HostMsg::GetEarliestBlockHeight { reply_to } => {
                let earliest_block_height = state.block_store.first_height().unwrap_or_default();
                reply_to.send(earliest_block_height)?;
                Ok(())
            }

            HostMsg::GetValue {
                height,
                round,
                timeout_duration,
                address: _,
                reply_to,
            } => {
                let deadline = Instant::now() + timeout_duration;

                debug!(%height, %round, "Building new proposal...");

                let (mut rx_part, rx_hash) =
                    state.host.build_new_proposal(height, round, deadline).await;

                let stream_id = state.next_stream_id();

                let mut sequence = 0;

                while let Some(part) = rx_part.recv().await {
                    state.host.part_store.store(height, round, part.clone());

                    if state.host.params.value_payload.include_parts() {
                        debug!(%stream_id, %sequence, "Broadcasting proposal part");

                        let msg = StreamMessage::new(
                            stream_id,
                            sequence,
                            StreamContent::Data(part.clone()),
                        );

                        self.gossip_consensus
                            .cast(GossipConsensusMsg::PublishProposalPart(msg))?;
                    }

                    sequence += 1;
                }

                if state.host.params.value_payload.include_parts() {
                    let msg = StreamMessage::new(stream_id, sequence, StreamContent::Fin(true));

                    self.gossip_consensus
                        .cast(GossipConsensusMsg::PublishProposalPart(msg))?;
                }

                let block_hash = rx_hash.await?;
                debug!(%block_hash, "Assembled block");

                state
                    .host
                    .part_store
                    .store_value_id(height, round, block_hash);

                let parts = state.host.part_store.all_parts(height, round);

                let Some((value, block)) =
                    state.build_block_from_parts(&parts, height, round).await
                else {
                    error!(%height, %round, "Failed to build block from parts");
                    return Ok(());
                };

                if let Err(e) = state
                    .block_store
                    .store_undecided_block(value.height, value.round, block)
                    .await
                {
                    error!(%e, %height, %round, "Failed to store the proposed block");
                }

                reply_to.send(LocallyProposedValue::new(
                    value.height,
                    value.round,
                    value.value,
                    value.extension,
                ))?;

                Ok(())
            }

            HostMsg::RestreamValue {
                height,
                round,
                valid_round,
                address,
                value_id,
            } => {
                debug!(%height, %round, "Restreaming existing proposal...");

                let mut rx_part = state.host.send_known_proposal(value_id).await;

                let stream_id = state.next_stream_id();

                let init = ProposalInit {
                    height,
                    proposal_round: round,
                    valid_round,
                    proposer: address.clone(),
                };

                let signature =
                    compute_proposal_signature(&init, &value_id, &state.host.private_key);

                let init_part = ProposalPart::Init(init);
                let fin_part = ProposalPart::Fin(ProposalFin { signature });

                debug!(%height, %round, "Created new Init part: {init_part:?}");

                let mut sequence = 0;

                while let Some(part) = rx_part.recv().await {
                    let new_part = match part.part_type() {
                        PartType::Init => init_part.clone(),
                        PartType::Fin => fin_part.clone(),
                        PartType::Transactions | PartType::BlockProof => part,
                    };

                    state.host.part_store.store(height, round, new_part.clone());

                    if state.host.params.value_payload.include_parts() {
                        debug!(%stream_id, %sequence, "Broadcasting proposal part");

                        let msg =
                            StreamMessage::new(stream_id, sequence, StreamContent::Data(new_part));

                        self.gossip_consensus
                            .cast(GossipConsensusMsg::PublishProposalPart(msg))?;

                        sequence += 1;
                    }
                }

                Ok(())
            }

            HostMsg::ReceivedProposalPart {
                from,
                part,
                reply_to,
            } => {
                // TODO - use state.host.receive_proposal() and move some of the logic below there
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

                    if let Some(value) = state
                        .build_value_from_part(parts.height, parts.round, part)
                        .await
                    {
                        reply_to.send(value)?;
                        break;
                    }
                }

                Ok(())
            }

            HostMsg::GetValidatorSet { height, reply_to } => {
                if let Some(validators) = state.host.validators(height).await {
                    reply_to.send(ValidatorSet::new(validators))?;
                    Ok(())
                } else {
                    Err(eyre!("No validator set found for the given height {height}").into())
                }
            }

            HostMsg::Decided {
                certificate,
                consensus,
            } => {
                let (height, round) = (certificate.height, certificate.round);

                let mut all_parts = state.host.part_store.all_parts(height, round);

                let mut all_txes = vec![];
                for part in all_parts.iter_mut() {
                    if let ProposalPart::Transactions(transactions) = part.as_ref() {
                        let mut txes = transactions.to_vec();
                        all_txes.append(&mut txes);
                    }
                }

                // Build the block from transaction parts and certificate, and store it
                if let Err(e) = state
                    .block_store
                    .store_decided_block(&certificate, &all_txes)
                    .await
                {
                    error!(%e, %height, %round, "Failed to store the block");
                }

                // Update metrics
                let block_size: usize = all_parts.iter().map(|p| p.size_bytes()).sum();
                let extension_size: usize = certificate
                    .aggregated_signature
                    .signatures
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

                // Gather hashes of all the tx-es included in the block,
                // so that we can notify the mempool to remove them.
                let mut tx_hashes = vec![];
                for part in all_parts {
                    if let ProposalPart::Transactions(txes) = &part.as_ref() {
                        tx_hashes.extend(txes.as_slice().iter().map(|tx| tx.hash()));
                    }
                }

                // Prune the PartStore of all parts for heights lower than `state.height`
                state.host.part_store.prune(state.height);

                // Store the block
                self.prune_block_store(state).await;

                // Notify the mempool to remove corresponding txs
                self.mempool.cast(MempoolMsg::Update { tx_hashes })?;

                // Notify Starknet Host of the decision
                state.host.decision(certificate).await;

                // Start the next height
                consensus.cast(ConsensusMsg::StartHeight(
                    state.height.increment(),
                    state.host.validator_set.clone(),
                ))?;

                Ok(())
            }

            HostMsg::GetDecidedBlock { height, reply_to } => {
                debug!(%height, "Received request for block");

                match state.block_store.get(height).await {
                    Ok(None) => {
                        let min = state.block_store.first_height().unwrap_or_default();
                        let max = state.block_store.last_height().unwrap_or_default();

                        warn!(%height, "No block for this height, available blocks: {min}..={max}");

                        reply_to.send(None)?;
                    }

                    Ok(Some(block)) => {
                        let block = SyncedBlock {
                            block_bytes: block.block.to_bytes().unwrap(),
                            certificate: block.certificate,
                        };

                        debug!(%height, "Found decided block in store");
                        reply_to.send(Some(block))?;
                    }
                    Err(e) => {
                        error!(%e, %height, "Failed to get decided block");
                        reply_to.send(None)?;
                    }
                }

                Ok(())
            }

            HostMsg::ProcessSyncedBlock {
                height,
                round,
                validator_address,
                block_bytes,
                reply_to,
            } => {
                let maybe_block = Block::from_bytes(block_bytes.as_ref());
                if let Ok(block) = maybe_block {
                    let proposed_value = ProposedValue {
                        height,
                        round,
                        valid_round: Round::Nil,
                        validator_address,
                        value: block.block_hash,
                        validity: Validity::Valid,
                        extension: None,
                    };

                    reply_to.send(proposed_value)?;
                }

                Ok(())
            }
        }
    }
}
