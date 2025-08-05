#![allow(clippy::too_many_arguments)]
use sha3::Digest;
use std::sync::Arc;
use std::time::SystemTime;

use bytesize::ByteSize;
use eyre::eyre;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tracing::{debug, error, trace};

use malachitebft_core_types::Round;
use malachitebft_signing_ed25519::PrivateKey;

use crate::fifo_mempool::{MempoolMsg, MempoolRef};
use crate::mock_host::MockHostParams;
use crate::types::{
    address::Address,
    hash::Hash,
    height::Height,
    proposal_part::{ProposalData, ProposalFin, ProposalInit, ProposalPart},
};

pub async fn build_proposal_task(
    height: Height,
    round: Round,
    proposer: Address,
    private_key: PrivateKey,
    params: MockHostParams,
    deadline: Instant,
    mempool: MempoolRef,
    tx_part: mpsc::Sender<ProposalPart>,
    tx_block_hash: oneshot::Sender<Hash>,
) {
    if let Err(e) = run_build_proposal_task(
        height,
        round,
        proposer,
        private_key,
        params,
        deadline,
        mempool,
        tx_part,
        tx_block_hash,
    )
    .await
    {
        error!("Failed to build proposal: {e:?}");
    }
}

async fn run_build_proposal_task(
    height: Height,
    round: Round,
    proposer: Address,
    private_key: PrivateKey,
    params: MockHostParams,
    deadline: Instant,
    mempool: MempoolRef,
    tx_part: mpsc::Sender<ProposalPart>,
    tx_value_id: oneshot::Sender<Hash>,
) -> Result<(), Box<dyn core::error::Error>> {
    // TODO - if needed, use this deadline to stop the build_new_proposal
    let start = Instant::now();
    let build_duration = (deadline - start).mul_f32(params.time_allowance_factor);

    let _build_deadline = start + build_duration;
    let _now = SystemTime::UNIX_EPOCH.elapsed().unwrap().as_secs();

    let mut sequence = 0;
    let mut block_tx_count = 0;
    let mut block_size = 0;

    trace!(%height, %round, "Building local value");

    // Init
    {
        let part = ProposalPart::Init(ProposalInit {
            height,
            round,
            proposer,
            valid_round: Round::Nil,
        });

        tx_part.send(part).await?;
        sequence += 1;
    }

    let max_block_size = params.max_block_size.as_u64() as usize;
    let mut hasher = sha3::Keccak256::new();

    'reap: loop {
        let reaped_txes = mempool
            .call(
                |reply| MempoolMsg::Take { reply },
                Some(build_duration),
            )
            .await?
            .success_or(eyre!("Failed to reap transactions from the mempool"))?;

        debug!("Reaped {} transactions from the mempool", reaped_txes.len());

        if reaped_txes.is_empty() {
            debug!("No more transactions to reap");
            break 'reap;
        }

        let mut txes = Vec::new();
        let mut full_block = false;

        'txes: for tx in reaped_txes {
            if block_size + tx.len() > max_block_size {
                full_block = true;
                break 'txes;
            }

            block_size += tx.len();
            block_tx_count += 1;
            txes.push(tx.clone());
            let tx_hash = tx.hash();
            hasher.update(tx_hash.0);
        }

        let exec_time = params.exec_time_per_tx * txes.len() as u32;
        tokio::time::sleep(exec_time).await;

        trace!(
            %sequence,
            "Created a tx batch with {} tx-es of size {} in {:?}",
            txes.len(),
            ByteSize::b(block_size as u64),
            start.elapsed()
        );

        // Transactions
        {
            let transactions: Vec<crate::types::transaction::Transaction> = txes
                .into_iter()
                .map(|raw_tx| crate::types::transaction::Transaction::new(raw_tx.0))
                .collect();
            let part = ProposalPart::Data(ProposalData { transactions });
            tx_part.send(part).await?;
            sequence += 1;
        }

        if full_block {
            debug!("Max block size reached, stopping tx generation");
            break 'reap;
        } else if start.elapsed() >= build_duration {
            debug!("Time allowance exceeded, stopping tx generation");
            break 'reap;
        }
    }

    let transaction_commitment = Hash::new(hasher.finalize().into());

    // Fin
    {
        let part = ProposalPart::Fin(ProposalFin {
            signature: private_key.sign(transaction_commitment.as_bytes().as_ref()),
            commitment: transaction_commitment.clone(),
        });
        tx_part.send(part).await?;
        sequence += 1;
    }

    // Close the channel to signal no more parts to come
    drop(tx_part);

    let block_size = ByteSize::b(block_size as u64);

    debug!(
        tx_count = %block_tx_count, size = %block_size, transaction_commitment = ?transaction_commitment, parts = %sequence,
        "Built block in {:?}", start.elapsed()
    );

    tx_value_id
        .send(transaction_commitment)
        .map_err(|_| "Failed to send proposal commitment hash")?;

    Ok(())
}

pub async fn repropose_task(
    block_hash: Hash,
    tx_part: mpsc::Sender<ProposalPart>,
    parts: Vec<Arc<ProposalPart>>,
) {
    if let Err(e) = run_repropose_task(block_hash, tx_part, parts).await {
        error!("Failed to restream proposal: {e:?}");
    }
}

async fn run_repropose_task(
    _block_hash: Hash,
    tx_part: mpsc::Sender<ProposalPart>,
    parts: Vec<Arc<ProposalPart>>,
) -> Result<(), Box<dyn core::error::Error>> {
    for part in parts {
        let part = Arc::unwrap_or_clone(part);
        tx_part.send(part).await?;
    }
    Ok(())
}
