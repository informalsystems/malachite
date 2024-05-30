use std::marker::PhantomData;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ractor::ActorRef;
use tracing::{error, info, trace};

use malachite_common::{Context, Round};

use crate::consensus::Msg as ConsensusMsg;
use crate::proposal_builder::{LocallyProposedValue, ReceivedProposedValue};
use crate::util::value_builder::test::PartStore;

#[async_trait]
pub trait ValueBuilder<Ctx: Context>: Send + Sync + 'static {
    async fn build_value_locally(
        &self,
        height: Ctx::Height,
        round: Round,
        timeout_duration: Duration,
        address: Ctx::Address,
        consensus: ActorRef<ConsensusMsg<Ctx>>,
        part_store: &mut PartStore,
    ) -> Option<LocallyProposedValue<Ctx>>;

    async fn build_value_from_block_parts(
        &self,
        block_part: Ctx::BlockPart,
        part_store: &mut PartStore,
    ) -> Option<ReceivedProposedValue<Ctx>>;

    async fn maybe_received_value(
        &self,
        height: Ctx::Height,
        round: Round,
        part_store: &mut PartStore,
    ) -> Option<ReceivedProposedValue<Ctx>>;
}

pub mod test {
    // TODO - parameterize
    // If based on the propose_timeout and the constants below we end up with more than 300 parts then consensus
    // is never reached in a round and we keep moving to the next one.
    const NUM_TXES_PER_PART: u64 = 400;
    const TIME_ALLOWANCE_FACTOR: f32 = 0.5;
    const EXEC_TIME_MICROSEC_PER_PART: u64 = 100000;

    use super::*;
    use std::collections::BTreeMap;

    use malachite_driver::Validity;
    use malachite_test::{
        Address, BlockMetadata, BlockPart, Content, Height, TestContext, TransactionBatch, Value,
    };
    use ractor::ActorRef;

    #[derive(Clone)]
    pub struct TestValueBuilder<Ctx: Context> {
        _phantom: PhantomData<Ctx>,
        tx_streamer: ActorRef<crate::mempool::Msg>,
    }

    impl<Ctx> TestValueBuilder<Ctx>
    where
        Ctx: Context,
    {
        pub fn new(tx_streamer: ActorRef<crate::mempool::Msg>) -> Self {
            Self {
                _phantom: Default::default(),
                tx_streamer,
            }
        }
    }

    #[async_trait]
    impl ValueBuilder<TestContext> for TestValueBuilder<TestContext> {
        async fn build_value_locally(
            &self,
            height: Height,
            round: Round,
            timeout_duration: Duration,
            validator_address: Address,
            consensus: ActorRef<ConsensusMsg<TestContext>>,
            part_store: &mut PartStore,
        ) -> Option<LocallyProposedValue<TestContext>> {
            let now = Instant::now();
            let deadline = now + timeout_duration.mul_f32(TIME_ALLOWANCE_FACTOR);
            let expiration_time = now + timeout_duration;

            let mut tx_batch = vec![];
            let mut sequence = 1;
            let mut result = None;

            loop {
                trace!(
                    "Build local value for h:{}, r:{}, s:{}",
                    height,
                    round,
                    sequence
                );

                let mut txes = self
                    .tx_streamer
                    .call(
                        |reply| crate::mempool::Msg::TxStream {
                            height: height.as_u64(),
                            num_txes: NUM_TXES_PER_PART,
                            reply,
                        },
                        None,
                    ) // TODO timeout
                    .await
                    .ok()?
                    .unwrap();

                if txes.is_empty() {
                    break;
                }

                // Create, store and gossip the batch in a BlockPart
                let block_part = BlockPart::new(
                    height,
                    round,
                    sequence,
                    validator_address,
                    Content::new(TransactionBatch::new(txes.clone()), None),
                );

                part_store.store(block_part.clone());

                consensus
                    .cast(ConsensusMsg::BuilderBlockPart(block_part.clone()))
                    .unwrap();

                // Simulate execution
                tokio::time::sleep(Duration::from_micros(EXEC_TIME_MICROSEC_PER_PART)).await;
                tx_batch.append(&mut txes);

                sequence += 1;

                if Instant::now() > expiration_time {
                    error!( "Value Builder started at {now:?} but failed to complete by expiration time {expiration_time:?}");
                    result = None;
                    break;
                }

                if Instant::now() > deadline {
                    // Create, store and gossip the BlockMetadata in a BlockPart
                    let value = Value::new_from_transactions(tx_batch.clone());

                    result = Some(LocallyProposedValue {
                        height,
                        round,
                        value: Some(value),
                    });

                    let block_part = BlockPart::new(
                        height,
                        round,
                        sequence,
                        validator_address,
                        Content::new(
                            TransactionBatch::new(vec![]),
                            Some(BlockMetadata::new(vec![], value)),
                        ),
                    );

                    part_store.store(block_part.clone());

                    consensus
                        .cast(ConsensusMsg::BuilderBlockPart(block_part.clone()))
                        .unwrap();

                    info!(
                        "Value Builder created a block with {} tx-es, block hash (consensus value) {:?} ",
                        tx_batch.len(),
                        result
                    );

                    break;
                }
            }

            result
        }

        async fn build_value_from_block_parts(
            &self,
            block_part: BlockPart,
            part_store: &mut PartStore,
        ) -> Option<ReceivedProposedValue<TestContext>> {
            let height = block_part.height();
            let round = block_part.round();
            let sequence = block_part.sequence();

            part_store.store(block_part.clone());
            let num_parts = part_store.all_parts(height, round).len();
            trace!("({num_parts}):Received block part (h: {height}, r: {round}, seq: {sequence}");

            // Simulate Tx execution and proof verification (assumes success)
            // TODO - add config knob for invalid blocks
            tokio::time::sleep(Duration::from_micros(EXEC_TIME_MICROSEC_PER_PART)).await;

            // Get the "last" part, the one with highest sequence.
            // Block parts may not be received in order.
            if let Some(last_part) =
                part_store.get(block_part.height(), block_part.round(), num_parts as u64)
            {
                // If the "last" part includes a metadata then this is truly the last part.
                // So in this case all block parts have been received, including the metadata that includes
                // the block hash/ value. This can be returned as the block is complete.
                // TODO - the logic here is weak, we assume earlier parts don't include metadata
                // Should change once we implement `oneof`/ proper enum in protobuf but good enough for now test code
                match last_part.metadata() {
                    Some(meta) => {
                        info!(
                            "Value Builder received last block part for height:{}, round:{}, num_parts: {num_parts}",
                            last_part.height(),
                            last_part.round(),
                        );
                        Some(ReceivedProposedValue {
                            validator_address: *last_part.validator_address(),
                            height: last_part.height(),
                            round: last_part.round(),
                            value: Some(meta.value()),
                            valid: Validity::Valid,
                        })
                    }
                    None => None,
                }
            } else {
                None
            }
        }

        async fn maybe_received_value(
            &self,
            height: Height,
            round: Round,
            part_store: &mut PartStore,
        ) -> Option<ReceivedProposedValue<TestContext>> {
            let block_parts = part_store.all_parts(height, round);
            let num_parts = block_parts.len();
            let last_part = block_parts[num_parts - 1];
            last_part.metadata().map(|metadata| ReceivedProposedValue {
                validator_address: *last_part.validator_address(),
                height,
                round,
                value: Some(metadata.value()),
                valid: Validity::Valid,
            })
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct PartStore {
        pub map: BTreeMap<(Height, Round, u64), BlockPart>,
    }

    impl Default for PartStore {
        fn default() -> Self {
            Self::new()
        }
    }

    impl PartStore {
        pub fn new() -> Self {
            Self {
                map: BTreeMap::new(),
            }
        }

        pub fn get(&self, height: Height, round: Round, sequence: u64) -> Option<&BlockPart> {
            self.map.get(&(height, round, sequence))
        }

        pub fn all_parts(&self, height: Height, round: Round) -> Vec<&BlockPart> {
            let mut block_parts: Vec<&BlockPart> = self
                .map
                .iter()
                .filter(|((h, r, _), _)| *h == height && *r == round)
                .map(|(_, b)| b)
                .collect();
            block_parts.sort_by_key(|b| std::cmp::Reverse(b.sequence()));
            block_parts
        }

        pub fn store(&mut self, block_part: BlockPart) {
            let height = block_part.height();
            let round = block_part.round();
            let sequence = block_part.sequence();
            self.map
                .entry((height, round, sequence))
                .or_insert(block_part);
        }
    }
}
