use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use bytesize::ByteSize;
use malachitebft_app::part_store::PartStore;
use malachitebft_core_types::{Round, CommitCertificate};
use malachitebft_signing_ed25519::PrivateKey;

use crate::mempool::MempoolRef;
use crate::types::address::Address;
use crate::types::context::MockContext;
use crate::types::validator_set::ValidatorSet;
use crate::types::height::Height;
use crate::types::proposal_part::ProposalPart;
use crate::types::value::ValueId;
use crate::types::hash::Hash;
use crate::proposal::{build_proposal_task, repropose_task};

#[derive(Copy, Clone, Debug)]
pub struct MockHostParams {
    pub max_block_size: ByteSize,
    pub txs_per_part: usize,
    pub time_allowance_factor: f32,
    pub exec_time_per_tx: Duration,
    pub max_retain_blocks: usize,
}

pub struct MockHost {
    pub params: MockHostParams,
    pub mempool: MempoolRef,
    pub address: Address,
    pub private_key: PrivateKey,
    pub validator_set: ValidatorSet,
    pub part_store: PartStore<MockContext>,
}

impl MockHost {
    pub fn new(
        params: MockHostParams,
        mempool: MempoolRef,
        address: Address,
        private_key: PrivateKey,
        validator_set: ValidatorSet,
    ) -> Self {
        Self {
            params,
            mempool,
            address,
            private_key,
            validator_set,
            part_store: Default::default(),
        }
    }

    #[tracing::instrument(skip_all, fields(%height, %round))]
    pub async fn build_new_proposal(
        &mut self,
        height: Height,
        round: Round,
        deadline: Instant,
    ) -> (
        mpsc::Receiver<ProposalPart>,
        oneshot::Receiver<Hash>,
    ) {
        let (tx_part, rx_content) = mpsc::channel(self.params.txs_per_part);
        let (tx_block_hash, rx_block_hash) = oneshot::channel();

        tokio::spawn(
            build_proposal_task(
                height,
                round,
                self.address,
                self.private_key.clone(),
                self.params,
                deadline,
                self.mempool.clone(),
                tx_part,
                tx_block_hash,
            )
        );

        (rx_content, rx_block_hash)
    }

    /// Send a proposal whose content is already known. LOC 16
    ///
    /// Params:
    /// - block_hash - Identifies the content to send.
    ///
    /// Returns:
    /// - content - A channel for sending the content of the proposal.
    #[tracing::instrument(skip_all, fields(%block_hash))]
    pub async fn send_known_proposal(
        &self,
        block_hash: ValueId,
    ) -> mpsc::Receiver<ProposalPart> {
        let parts = self.part_store.all_parts_by_value_id(&block_hash);
        let (tx_part, rx_content) = mpsc::channel(self.params.txs_per_part);

        tokio::spawn(
            repropose_task(block_hash.as_hash().clone(), tx_part, parts),
        );

        rx_content
    }

    /// Update the Context about which decision has been made. It is responsible for pinging any
    /// relevant components in the node to update their states accordingly.
    ///
    /// Params:
    /// - brock_hash - The ID of the content which has been decided.
    /// - precommits - The list of precommits from the round the decision was made (both for and against).
    /// - height     - The height of the decision.
    #[tracing::instrument(skip_all, fields(height = %_certificate.height, block_hash = %_certificate.value_id))]
    pub async fn decision(&self, _certificate: CommitCertificate<MockContext>) {}
}
