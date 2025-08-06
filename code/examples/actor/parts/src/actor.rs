use std::path::PathBuf;
use std::time::Duration;

use bytes::Bytes;
use itertools::Itertools;
use ractor::{async_trait, Actor, ActorProcessingErr, RpcReplyPort, SpawnErr};
use rand::rngs::StdRng;
use rand::SeedableRng;
use tokio::time::Instant;
use tracing::{debug, error, info, trace, warn};

use crate::types::signing::Ed25519Provider;
use crate::types::value::Value;
use malachitebft_core_consensus::{PeerId, Role};
use malachitebft_core_types::{CommitCertificate, Round, VoteExtensions};
use malachitebft_engine::consensus::ConsensusMsg;
use malachitebft_engine::host::{LocallyProposedValue, ProposedValue};
use malachitebft_engine::network::{NetworkMsg, NetworkRef};
use malachitebft_engine::util::streaming::{StreamContent, StreamMessage};
use malachitebft_proto::Protobuf;
use malachitebft_sync::RawDecidedValue;

use crate::fifo_mempool::{MempoolMsg, MempoolRef};
use crate::mempool_load::MempoolLoadRef;
use crate::metrics::Metrics;
use crate::mock_host::MockHost;
use crate::state::HostState;
use crate::types::address::Address;
use crate::types::block::Block;
use crate::types::context::MockContext;
use crate::types::height::Height;
use crate::types::proposal_part::{PartType, ProposalFin, ProposalInit, ProposalPart};
use crate::types::validator_set::ValidatorSet;
use crate::types::value::ValueId;

pub struct Host {
    mempool: MempoolRef,
    mempool_load: MempoolLoadRef,
    network: NetworkRef<MockContext>,
    metrics: Metrics,
    span: tracing::Span,
}

pub type HostRef = malachitebft_engine::host::HostRef<MockContext>;
pub type HostMsg = malachitebft_engine::host::HostMsg<MockContext>;

impl Host {
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn(
        home_dir: PathBuf,
        signing_provider: Ed25519Provider,
        host: MockHost,
        mempool: MempoolRef,
        mempool_load: MempoolLoadRef,
        network: NetworkRef<MockContext>,
        metrics: Metrics,
        span: tracing::Span,
    ) -> Result<HostRef, SpawnErr> {
        let db_dir = home_dir.join("db");
        std::fs::create_dir_all(&db_dir).map_err(|e| SpawnErr::StartupFailed(e.into()))?;
        let db_path = db_dir.join("blocks.db");

        let ctx = MockContext::new();

        let (actor_ref, _) = Actor::spawn(
            None,
            Self::new(mempool, mempool_load, network, metrics, span),
            HostState::new(
                ctx,
                signing_provider,
                host,
                db_path,
                &mut StdRng::from_entropy(),
            )
            .await,
        )
        .await?;

        Ok(actor_ref)
    }

    pub fn new(
        mempool: MempoolRef,
        mempool_load: MempoolLoadRef,
        network: NetworkRef<MockContext>,
        metrics: Metrics,
        span: tracing::Span,
    ) -> Self {
        Self {
            mempool,
            mempool_load,
            network,
            metrics,
            span,
        }
    }
}

#[async_trait]
impl Actor for Host {
    type Arguments = HostState;
    type State = HostState;
    type Msg = HostMsg;

    async fn pre_start(
        &self,
        myself: HostRef,
        initial_state: Self::State,
    ) -> Result<Self::State, ActorProcessingErr> {
        self.mempool.link(myself.get_cell());
        self.mempool_load.link(myself.get_cell());

        Ok(initial_state)
    }

    async fn handle(
        &self,
        _myself: HostRef,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if let Err(e) = self.handle_msg(_myself, msg, state).await {
            error!(%e, "Failed to handle message");
        }

        Ok(())
    }
}

impl Host {
    #[tracing::instrument(
        name = "host",
        parent = &self.span,
        skip_all,
        fields(height = %state.height, round = %state.round),
    )]
    async fn handle_msg(
        &self,
        _myself: HostRef,
        msg: HostMsg,
        state: &mut HostState,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            HostMsg::ConsensusReady { reply_to } => on_consensus_ready(state, reply_to).await,

            HostMsg::StartedRound {
                height,
                round,
                proposer,
                role,
                reply_to,
            } => on_started_round(state, height, round, proposer, role, reply_to).await,

            HostMsg::GetHistoryMinHeight { reply_to } => {
                on_get_history_min_height(state, reply_to).await
            }

            HostMsg::GetValue {
                height,
                round,
                timeout,
                reply_to,
            } => on_get_value(state, &self.network, height, round, timeout, reply_to).await,

            HostMsg::RestreamValue {
                height,
                round,
                valid_round,
                address,
                value_id,
            } => {
                on_restream_proposal(
                    state,
                    &self.network,
                    height,
                    round,
                    value_id,
                    valid_round,
                    address,
                )
                .await
            }

            HostMsg::ReceivedProposalPart {
                from,
                part,
                reply_to,
            } => on_received_proposal_part(state, part, from, reply_to).await,

            HostMsg::GetValidatorSet { height, reply_to } => {
                on_get_validator_set(state, height, reply_to).await
            }

            HostMsg::Decided {
                certificate,
                extensions,
                reply_to,
            } => {
                on_decided(
                    state,
                    &self.mempool,
                    certificate,
                    extensions,
                    reply_to,
                    &self.metrics,
                )
                .await
            }

            HostMsg::GetDecidedValue { height, reply_to } => {
                on_get_decided_block(height, state, reply_to).await
            }

            HostMsg::ProcessSyncedValue {
                height,
                round,
                proposer,
                value_bytes,
                reply_to,
            } => {
                on_process_synced_value(state, value_bytes, height, round, proposer, reply_to).await
            }

            HostMsg::ExtendVote { reply_to, .. } => {
                reply_to.send(None)?;
                Ok(())
            }

            HostMsg::VerifyVoteExtension { reply_to, .. } => {
                reply_to.send(Ok(()))?;
                Ok(())
            }
        }
    }
}

async fn on_consensus_ready(
    state: &mut HostState,
    reply_to: RpcReplyPort<(Height, ValidatorSet)>,
) -> Result<(), ActorProcessingErr> {
    let latest_block_height = state
        .block_store
        .max_decided_value_height()
        .await
        .unwrap_or_default();
    let start_height = latest_block_height.increment();

    tokio::time::sleep(Duration::from_millis(200)).await;

    reply_to.send((start_height, state.host.validator_set.clone()))?;
    Ok(())
}

async fn on_started_round(
    state: &mut HostState,
    height: Height,
    round: Round,
    proposer: Address,
    role: Role,
    reply_to: RpcReplyPort<Vec<ProposedValue<MockContext>>>,
) -> Result<(), ActorProcessingErr> {
    state.height = height;
    state.round = round;
    state.proposer = Some(proposer);
    state.role = role;

    info!(%height, %round, %proposer, ?role, "Started new round");

    let pending = state
        .block_store
        .get_pending_proposals(height, round)
        .await?;
    info!(%height, %round, "Found {} pending proposals, validating...", pending.len());
    for p in &pending {
        // TODO: check proposal validity
        state
            .block_store
            .store_undecided_proposal(p.clone())
            .await?;
        state.block_store.remove_pending_proposal(p.clone()).await?;
    }

    // If we have already built or seen one or more values for this height and round,
    // feed them back to consensus. This may happen when we are restarting after a crash.
    let undecided_values = state
        .block_store
        .get_undecided_proposals(height, round)
        .await?;

    info!(%height, %round, "Found {} undecided values", undecided_values.len());
    reply_to.send(undecided_values)?;

    Ok(())
}

async fn on_get_history_min_height(
    state: &mut HostState,
    reply_to: RpcReplyPort<Height>,
) -> Result<(), ActorProcessingErr> {
    let history_min_height = state
        .block_store
        .min_decided_value_height()
        .await
        .unwrap_or_default();
    reply_to.send(history_min_height)?;

    Ok(())
}

async fn on_get_validator_set(
    state: &mut HostState,
    _height: Height,
    reply_to: RpcReplyPort<Option<ValidatorSet>>,
) -> Result<(), ActorProcessingErr> {
    reply_to.send(Some(state.host.validator_set.clone()))?;
    Ok(())
}

async fn on_get_value(
    state: &mut HostState,
    network: &NetworkRef<MockContext>,
    height: Height,
    round: Round,
    timeout: Duration,
    reply_to: RpcReplyPort<LocallyProposedValue<MockContext>>,
) -> Result<(), ActorProcessingErr> {
    if let Some(value) = find_previously_built_value(state, height, round).await? {
        info!(%height, %round, hash = ?value.value, "Returning previously built value");

        reply_to.send(LocallyProposedValue::new(
            value.height,
            value.round,
            value.value,
        ))?;

        return Ok(());
    }

    let deadline = Instant::now() + timeout;

    debug!(%height, %round, "Building new proposal...");

    let (mut rx_part, rx_hash) = state.host.build_new_proposal(height, round, deadline).await;

    let stream_id = state.stream_id();

    let mut sequence = 0;

    while let Some(part) = rx_part.recv().await {
        state
            .host
            .part_store
            .store(&stream_id, height, round, part.clone());

        debug!(%stream_id, %sequence, "Broadcasting proposal part");

        let msg = StreamMessage::new(
            stream_id.clone(),
            sequence,
            StreamContent::Data(part.clone()),
        );
        network.cast(NetworkMsg::PublishProposalPart(msg))?;

        sequence += 1;
    }

    let msg = StreamMessage::new(stream_id.clone(), sequence, StreamContent::Fin);
    network.cast(NetworkMsg::PublishProposalPart(msg))?;

    let block_hash = rx_hash.await?;
    debug!(%height, %round, block_hash = ?block_hash.clone(), "Storing proposed value from assembled block");

    state.host.part_store.store_value_id(
        &stream_id,
        height,
        round,
        ValueId::new(block_hash.clone()),
    );

    let parts = state
        .host
        .part_store
        .all_parts_by_stream_id(stream_id, height, round);

    let value = state
        .build_proposal_from_parts(height, round, &parts)
        .await?;

    debug!(%height, %round, block_hash = ?block_hash.clone(), "Storing proposed value from assembled block");
    if let Err(e) = state
        .block_store
        .store_undecided_proposal(value.clone())
        .await
    {
        error!(%e, %height, %round, "Failed to store the proposed value");
    }

    reply_to.send(LocallyProposedValue::new(
        value.height,
        value.round,
        value.value,
    ))?;

    Ok(())
}

/// If we have already built a block for this height and round, return it to consensus
/// This may happen when we are restarting after a crash and replaying the WAL.
async fn find_previously_built_value(
    state: &mut HostState,
    height: Height,
    round: Round,
) -> Result<Option<ProposedValue<MockContext>>, ActorProcessingErr> {
    let values = state
        .block_store
        .get_undecided_proposals(height, round)
        .await?;

    let proposed_value = values
        .into_iter()
        .find(|v| v.proposer == state.host.address);

    Ok(proposed_value)
}

async fn on_restream_proposal(
    state: &mut HostState,
    network: &NetworkRef<MockContext>,
    height: Height,
    round: Round,
    value_id: ValueId,
    valid_round: Round,
    proposer: Address,
) -> Result<(), ActorProcessingErr> {
    debug!(%height, %round, "Restreaming existing proposal...");

    let mut rx_part = state.host.send_known_proposal(value_id.clone()).await;

    let stream_id = state.stream_id();

    let init = ProposalInit {
        height,
        round,
        valid_round,
        proposer,
    };

    let init_part = ProposalPart::Init(init);
    let hash = value_id.clone().to_bytes().unwrap();
    let signature = state.signing_provider.sign(&hash);
    let fin_part = ProposalPart::Fin(ProposalFin {
        signature,
        commitment: value_id.as_hash().clone(),
    });

    debug!(%height, %round, "Created new Init part: {init_part:?}");

    let mut sequence = 0;

    while let Some(part) = rx_part.recv().await {
        let new_part = match part.part_type() {
            PartType::Init => init_part.clone(),
            PartType::Data => part.clone(),
            PartType::Fin => fin_part.clone(),
        };

        state
            .host
            .part_store
            .store(&stream_id, height, round, new_part.clone());

        debug!(%stream_id, %sequence, "Broadcasting proposal part");

        let msg = StreamMessage::new(stream_id.clone(), sequence, StreamContent::Data(new_part));

        network.cast(NetworkMsg::PublishProposalPart(msg))?;

        sequence += 1;
    }

    Ok(())
}

async fn on_process_synced_value(
    state: &mut HostState,
    value_bytes: Bytes,
    height: Height,
    round: Round,
    proposer: Address,
    reply_to: RpcReplyPort<ProposedValue<MockContext>>,
) -> Result<(), ActorProcessingErr> {
    let maybe_block = Block::from_bytes(value_bytes.as_ref());
    if let Ok(block) = maybe_block {
        let block_hash = block.block_hash.clone();
        let transactions = block.transactions.to_vec();
        let validity = state.verify_proposal_validity(&block_hash, transactions);

        state
            .block_store
            .store_undecided_block(height, round, block)
            .await?;

        let proposed_value = ProposedValue {
            height,
            round,
            valid_round: Round::Nil,
            proposer,
            value: Value::new(block_hash),
            validity,
        };

        state
            .block_store
            .store_undecided_proposal(proposed_value.clone())
            .await?;
        reply_to.send(proposed_value)?;
    }

    Ok(())
}

async fn on_get_decided_block(
    height: Height,
    state: &mut HostState,
    reply_to: RpcReplyPort<Option<RawDecidedValue<MockContext>>>,
) -> Result<(), ActorProcessingErr> {
    debug!(%height, "Received request for block");

    match state.block_store.get_decided_value(height).await {
        Ok(None) => {
            let min = state
                .block_store
                .min_decided_value_height()
                .await
                .unwrap_or_default();
            let max = state
                .block_store
                .max_decided_value_height()
                .await
                .unwrap_or_default();

            warn!(%height, "No block for this height, available blocks: {min}..={max}");

            reply_to.send(None)?;
        }

        Ok(Some(block)) => {
            let block = RawDecidedValue {
                value_bytes: block.block.to_bytes().unwrap(),
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

/// This function handles receiving parts of proposals
/// And assembling proposal in right sequence when all parts are collected
///
/// For each proposal from distinct peer separate stream is opened
/// Bookkeeping of streams is done inside HostState.part_streams_map ((peerId + streamId) -> streamState)
async fn on_received_proposal_part(
    state: &mut HostState,
    part: StreamMessage<ProposalPart>,
    from: PeerId,
    reply_to: RpcReplyPort<ProposedValue<MockContext>>,
) -> Result<(), ActorProcessingErr> {
    // When inserting part in a map, stream tries to connect all received parts in the right order,
    // starting from beginning and emits parts sequence chunks  when it succeeds
    // If it can't connect part, it buffers it
    // E.g. buffered: 1 3 7
    // part 0 (first) arrives -> 0 and 1 are emitted
    // 4 arrives -> gets buffered
    // 2 arrives -> 2, 3 and 4 are emitted

    // `insert` returns connected sequence of parts if any is emitted
    // If all parts have been received the stream is removed from the map streams
    let Some(parts) = state.part_streams_map.insert(from, part.clone()) else {
        return Ok(());
    };

    // The `part` sequence number must be for the first `ProposalPart` in `parts`.
    // So we start with this sequence and we increment for the debug log.
    let mut sequence = part.sequence;
    let stream_id = part.stream_id.clone();

    if parts.height < state.height {
        trace!(
            height = %state.height,
            round = %state.round,
            part.height = %parts.height,
            part.round = %parts.round,
            part.sequence = %sequence,
            "Received outdated proposal part for past height, ignoring"
        );

        return Ok(());
    }

    // Emitted parts are stored and simulated (if it is tx)
    // When finish part is stored, proposal value is built from all of them
    for part in parts.parts {
        debug!(
            part.sequence = %sequence,
            part.height = %parts.height,
            part.round = %parts.round,
            part.message = ?part.part_type(),
            "Processing proposal part"
        );

        if let Some(value) = state
            .build_value_from_part(&stream_id, parts.height, parts.round, part)
            .await
        {
            if let Some(value) = store_proposed_value(state, parts.height, value).await? {
                // Value is for current height, so we can send it to consensus
                reply_to.send(value)?;
            }

            break;
        }

        sequence += 1;
    }

    Ok(())
}

/// Store the proposed value in the block store.
///
/// If the height of the proposed value is greater than the current height,
/// store it as a pending value and return `None`.
///
/// If the height is equal to the current height, store it as an undecided value and return
/// `Some(value)`.
async fn store_proposed_value(
    state: &mut HostState,
    height: Height,
    value: ProposedValue<MockContext>,
) -> Result<Option<ProposedValue<MockContext>>, ActorProcessingErr> {
    debug!(
        height = %value.height,
        round = %value.round,
        block_hash = ?value.value,
        validity = ?value.validity,
        "Storing proposed value assembled from proposal parts"
    );

    if height > state.height {
        if let Err(e) = state
            .block_store
            .store_pending_proposal(value.clone())
            .await
        {
            error!(
                %e, height = %value.height, round = %value.round, block_hash = ?value.value,
                "Failed to store the pending proposed value"
            );
        }

        Ok(None)
    } else {
        if let Err(e) = state
            .block_store
            .store_undecided_proposal(value.clone())
            .await
        {
            error!(
                %e, height = %value.height, round = %value.round, block_hash = ?value.value,
                "Failed to store the undecided proposed value"
            );
        }

        Ok(Some(value))
    }
}

async fn on_decided(
    state: &mut HostState,
    mempool: &MempoolRef,
    certificate: CommitCertificate<MockContext>,
    _extensions: VoteExtensions<MockContext>,
    reply_to: RpcReplyPort<malachitebft_engine::host::Next<MockContext>>,
    metrics: &Metrics,
) -> Result<(), ActorProcessingErr> {
    let (height, round) = (certificate.height, certificate.round);

    let blocks = state
        .block_store
        .get_undecided_blocks(height, round)
        .await?;
    let Some(block) = blocks
        .iter()
        .find(|block| block.block_hash == *certificate.value_id.as_hash())
    else {
        error!(%height, %round, "Block not found");
        return Ok(());
    };

    // Store the block in the decided blocks table
    if let Err(e) = state
        .block_store
        .store_decided_block(&certificate, block.clone())
        .await
    {
        error!(%e, %height, %round, "Failed to store the block");
        return Ok(());
    }

    // Update metrics
    let tx_count: usize = block.transactions.to_vec().len();
    let block_size: usize = block.to_bytes().unwrap().len();

    metrics.block_tx_count.observe(tx_count as f64);
    metrics.block_size_bytes.observe(block_size as f64);
    metrics.finalized_txes.inc_by(tx_count as u64);

    // Convert transactions to RawTx and get their hashes
    let raw_txs: Vec<::mempool::RawTx> = block
        .transactions
        .to_vec()
        .into_iter()
        .map(|tx| ::mempool::RawTx(tx.to_bytes()))
        .collect();

    // Prune the PartStore of all parts for heights lower than `state.height`
    state.host.part_store.prune(state.height);

    // Prune the block store, keeping only the last `max_retain_blocks` blocks.
    // Also prunes parts, the undecided blocks and proposals.
    prune_block_store(state).await;

    // Notify the mempool to remove corresponding txs by hash
    for raw_tx in raw_txs {
        let tx_hash = raw_tx.hash();
        mempool.cast(MempoolMsg::Remove(vec![tx_hash]))?;
    }

    // Notify the Host of the decision
    state.host.decision(certificate).await;

    // Start the next height
    if let Some(consensus) = &state.consensus {
        consensus.cast(ConsensusMsg::StartHeight(
            state.height.increment(),
            state.host.validator_set.clone(),
        ))?;
    }

    // Reply with Next::Start
    reply_to.send(malachitebft_engine::host::Next::Start(
        state.height.increment(),
        state.host.validator_set.clone(),
    ))?;

    Ok(())
}

async fn prune_block_store(state: &mut HostState) {
    let max_height = state
        .block_store
        .max_decided_value_height()
        .await
        .unwrap_or_default();
    let max_retain_blocks = state.host.params.max_retain_blocks as u64;

    // Compute the height to retain blocks higher than
    let retain_height = max_height.as_u64().saturating_sub(max_retain_blocks);
    if retain_height <= 1 {
        // No need to prune anything, since we would retain every blocks
        return;
    }

    let retain_height = Height::new(retain_height);
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
