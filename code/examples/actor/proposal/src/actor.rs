use sha3::Digest;
use std::path::PathBuf;
use std::time::Duration;

use bytes::Bytes;
use itertools::Itertools;
use ractor::{async_trait, Actor, ActorProcessingErr, RpcReplyPort, SpawnErr};
use rand::rngs::StdRng;
use rand::SeedableRng;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

use crate::types::hash::Hash;
use crate::types::signing::Ed25519Provider;
use crate::types::value::Value;
use malachitebft_core_consensus::Role;
use malachitebft_core_types::{CommitCertificate, Round, Validity, ValueOrigin};
use malachitebft_engine::consensus::{ConsensusMsg, ConsensusRef};
use malachitebft_engine::host::{LocallyProposedValue, ProposedValue};
use malachitebft_metrics::Metrics;
use malachitebft_proto::Protobuf;
use malachitebft_sync::RawDecidedValue;

use crate::mempool::{MempoolMsg, MempoolRef};
use crate::mempool_load::MempoolLoadRef;
use crate::mock_host::MockHost;
use crate::state::HostState;
use crate::types::address::Address;
use crate::types::block::Block;
use crate::types::context::MockContext;
use crate::types::height::Height;
use crate::types::validator_set::ValidatorSet;

pub struct Host {
    mempool: MempoolRef,
    mempool_load: MempoolLoadRef,
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
        metrics: Metrics,
        span: tracing::Span,
    ) -> Result<HostRef, SpawnErr> {
        let db_dir = home_dir.join("db");
        std::fs::create_dir_all(&db_dir).map_err(|e| SpawnErr::StartupFailed(e.into()))?;
        let db_path = db_dir.join("blocks.db");

        let ctx = MockContext::new();

        let (actor_ref, _) = Actor::spawn(
            None,
            Self::new(mempool, mempool_load, metrics, span),
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
        metrics: Metrics,
        span: tracing::Span,
    ) -> Self {
        Self {
            mempool,
            mempool_load,
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
            HostMsg::ConsensusReady(consensus) => on_consensus_ready(state, consensus).await,

            HostMsg::StartedRound {
                height,
                round,
                proposer,
                role,
            } => on_started_round(state, height, round, proposer, role).await,

            HostMsg::GetHistoryMinHeight { reply_to } => {
                on_get_history_min_height(state, reply_to).await
            }

            HostMsg::GetValue {
                height,
                round,
                timeout,
                reply_to,
            } => on_get_value(state, height, round, timeout, reply_to).await,

            HostMsg::RestreamValue { .. } => {
                panic!("RestreamValue is not supported for ProposalOnly mode")
            }

            HostMsg::ReceivedProposalPart { .. } => {
                panic!("ReceivedProposalPart is not supported for ProposalOnly mode")
            }

            HostMsg::GetValidatorSet { height, reply_to } => {
                on_get_validator_set(state, height, reply_to).await
            }

            HostMsg::Decided {
                certificate,
                consensus,
                ..
            } => on_decided(state, &consensus, &self.mempool, certificate, &self.metrics).await,

            HostMsg::GetDecidedValue { height, reply_to } => {
                on_get_decided_block(height, state, reply_to).await
            }

            HostMsg::ProcessSyncedValue {
                height,
                round,
                validator_address,
                value_bytes,
                reply_to,
            } => {
                on_process_synced_value(value_bytes, height, round, validator_address, reply_to)
                    .await
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
    consensus: ConsensusRef<MockContext>,
) -> Result<(), ActorProcessingErr> {
    let latest_block_height = state
        .block_store
        .max_decided_value_height()
        .await
        .unwrap_or_default();
    let start_height = latest_block_height.increment();

    state.consensus = Some(consensus.clone());

    tokio::time::sleep(Duration::from_millis(200)).await;

    consensus.cast(ConsensusMsg::StartHeight(
        start_height,
        state.host.validator_set.clone(),
    ))?;

    Ok(())
}

async fn replay_undecided_values(
    state: &mut HostState,
    height: Height,
    round: Round,
) -> Result<(), ActorProcessingErr> {
    let undecided_values = state
        .block_store
        .get_undecided_proposals(height, round)
        .await?;

    let consensus = state.consensus.as_ref().unwrap();

    for proposal in undecided_values {
        let hash = proposal.value.value.block_hash.clone();
        info!(%height, %round, hash = ?hash, "Replaying already known proposed value");

        consensus.cast(ConsensusMsg::ReceivedProposedValue(
            proposal,
            ValueOrigin::Consensus,
        ))?;
    }

    Ok(())
}

async fn on_started_round(
    state: &mut HostState,
    height: Height,
    round: Round,
    proposer: Address,
    role: Role,
) -> Result<(), ActorProcessingErr> {
    state.height = height;
    state.round = round;
    state.proposer = Some(proposer);
    state.role = role;

    // If we have already built or seen one or more values for this height and round,
    // feed them back to consensus. This may happen when we are restarting after a crash.
    replay_undecided_values(state, height, round).await?;

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
    height: Height,
    round: Round,
    timeout: Duration,
    reply_to: RpcReplyPort<LocallyProposedValue<MockContext>>,
) -> Result<(), ActorProcessingErr> {
    if let Some(value) = find_previously_built_value(state, height, round).await? {
        info!(%height, %round, hash = ?value.value.value.block_hash, "Returning previously built value");

        reply_to.send(LocallyProposedValue::new(
            value.height,
            value.round,
            value.value,
        ))?;

        return Ok(());
    }

    let deadline = Instant::now() + timeout;

    debug!(%height, %round, "Building new proposal...");

    let block = state
        .host
        .build_new_proposal(height, round, deadline)
        .await?;

    let proposal = ProposedValue {
        height,
        round,
        valid_round: Round::Nil,
        proposer: state.host.address,
        value: Value::new(block.clone()),
        validity: Validity::Valid,
    };
    debug!(%height, %round, block_hash = ?block.block_hash, "Storing proposed value from assembled block");

    if let Err(e) = state.block_store.store_undecided_proposal(proposal).await {
        error!(%e, %height, %round, "Failed to store the proposed value");
    }

    reply_to.send(LocallyProposedValue::new(height, round, Value::new(block)))?;

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

async fn on_process_synced_value(
    value_bytes: Bytes,
    height: Height,
    round: Round,
    proposer: Address,
    reply_to: RpcReplyPort<ProposedValue<MockContext>>,
) -> Result<(), ActorProcessingErr> {
    let maybe_block = Block::from_bytes(value_bytes.as_ref());
    if let Ok(block) = maybe_block {
        let validity = verify_proposal_validity(&block).await;
        let proposed_value = ProposedValue {
            height,
            round,
            valid_round: Round::Nil,
            proposer,
            value: Value::new(block),
            validity,
        };

        reply_to.send(proposed_value)?;
    }

    Ok(())
}

async fn verify_proposal_validity(block: &Block) -> Validity {
    let mut hasher = sha3::Keccak256::new();

    for tx in block.transactions.to_vec().iter() {
        hasher.update(tx.hash().as_bytes());
    }

    let transaction_commitment = Hash::new(hasher.finalize().into());

    let valid_proposal = transaction_commitment == block.block_hash;

    if valid_proposal {
        Validity::Valid
    } else {
        error!(
            "ProposalCommitment hash mismatch: {:?} != {:?}",
            transaction_commitment, block.block_hash
        );
        Validity::Invalid
    }
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

async fn on_decided(
    state: &mut HostState,
    consensus: &ConsensusRef<MockContext>,
    mempool: &MempoolRef,
    certificate: CommitCertificate<MockContext>,
    metrics: &Metrics,
) -> Result<(), ActorProcessingErr> {
    let (height, round) = (certificate.height, certificate.round);

    let proposals = state
        .block_store
        .get_undecided_proposals(height, round)
        .await?;

    let Some(proposal) = proposals
        .into_iter()
        .find(|p| p.value.id() == certificate.value_id)
    else {
        error!(
            value_id = ?certificate.value_id,
            height = ?height,
            round = ?round,
            "Trying to commit a value for which there is no proposal"
        );
        return Ok(());
    };

    let block = proposal.value.value;
    if let Err(e) = state
        .block_store
        .store_decided_block(&certificate, &block)
        .await
    {
        error!(%e, %height, %round, "Failed to store the block");
    }

    // Update metrics
    let tx_count: usize = block.transactions.len();
    let block_size: usize = block.size_bytes();

    metrics.block_tx_count.observe(tx_count as f64);
    metrics.block_size_bytes.observe(block_size as f64);
    metrics.finalized_txes.inc_by(tx_count as u64);

    // Gather hashes of all the tx-es included in the block,
    // so that we can notify the mempool to remove them.
    let mut tx_hashes = vec![];
    for tx in block.transactions.to_vec().iter() {
        tx_hashes.push(tx.hash().clone());
    }

    // Prune the block store, keeping only the last `max_retain_blocks` blocks
    prune_block_store(state).await;

    // Notify the mempool to remove corresponding txs
    mempool.cast(MempoolMsg::Update { tx_hashes })?;

    // Notify the Host of the decision
    state.host.decision(certificate).await;

    // Start the next height
    consensus.cast(ConsensusMsg::StartHeight(
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
