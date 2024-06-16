#![allow(clippy::too_many_arguments)]

use bytesize::ByteSize;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tracing::{error, trace};

use malachite_actors::mempool::{MempoolMsg, MempoolRef};
use malachite_common::Round;

use crate::mock::host::MockParams;
use crate::mock::types::*;

pub async fn build_proposal_task(
    height: Height,
    round: Round,
    params: MockParams,
    deadline: Instant,
    mempool: MempoolRef,
    tx_part: mpsc::Sender<ProposalPart>,
    tx_block_hash: oneshot::Sender<BlockHash>,
) {
    if let Err(e) = run_build_proposal_task(
        height,
        round,
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
    params: MockParams,
    deadline: Instant,
    mempool: MempoolRef,
    tx_part: mpsc::Sender<ProposalPart>,
    tx_block_hash: oneshot::Sender<BlockHash>,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let build_duration = (deadline - start).mul_f32(params.time_allowance_factor);

    let mut tx_batch = Vec::new();
    let mut sequence = 1;
    let mut block_size = 0;
    let mut block_tx_count = 0;
    let mut block_hasher = Sha256::new();

    loop {
        trace!(%height, %round, %sequence, "Building local value");

        let txes = mempool
            .call(
                |reply| MempoolMsg::TxStream {
                    height: height.as_u64(),
                    num_txes: params.txs_per_part,
                    reply,
                },
                Some(build_duration),
            )
            .await?
            .success_or("Failed to get tx-es from the mempool")?;

        trace!("Reaped {} tx-es from the mempool", txes.len());

        if txes.is_empty() {
            break;
        }

        let mut tx_count = 0;

        'inner: for tx in txes {
            if block_size + tx.size_bytes() > params.max_block_size.as_u64() as usize {
                break 'inner;
            }

            block_size += tx.size_bytes();
            block_hasher.update(tx.as_bytes());
            tx_batch.push(tx);
            tx_count += 1;
        }

        tokio::time::sleep(tx_count * params.exec_time_per_tx).await;

        if start.elapsed() > build_duration {
            trace!("Time allowance exceeded, stopping tx generation");
            break;
        }

        if tx_count == 0 {
            trace!("No tx-es fit in the block, stopping tx generation");
            break;
        }

        block_tx_count += tx_count;

        trace!(
            "Created a tx batch with {} tx-es of size {} in {:?}",
            tx_batch.len(),
            ByteSize::b(tx_batch.iter().map(|tx| tx.size_bytes()).sum::<usize>() as u64),
            start.elapsed()
        );

        sequence += 1;

        let part = ProposalPart::TxBatch(
            sequence,
            TransactionBatch::new(std::mem::take(&mut tx_batch)),
        );

        tx_part.send(part).await?;

        if start.elapsed() > build_duration || block_size >= params.max_block_size.as_u64() as usize
        {
            break;
        }
    }

    // TODO: Compute actual "proof"
    let proof = vec![42];

    let hash = block_hasher.finalize();
    let block_hash = BlockHash::new(hash.into());
    let block_metadata = BlockMetadata::new(proof, block_hash);
    let part = ProposalPart::Metadata(sequence + 1, block_metadata);
    let block_size = ByteSize::b(block_size as u64);

    trace!("Built block with {block_tx_count} tx-es of size {block_size} and hash: {block_hash}");

    // Send and then close the channel
    tx_part.send(part).await?;
    drop(tx_part);

    tx_block_hash
        .send(block_hash)
        .map_err(|_| "Failed to send block hash")?;

    Ok(())
}
