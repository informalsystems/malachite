use eyre::eyre;
use std::time::Duration;

use async_trait::async_trait;
use bytesize::ByteSize;
use mempool::{CheckTxOutcome, TxHash};
use ractor::{concurrency::JoinHandle, Actor, ActorProcessingErr, ActorRef};
use rand::rngs::SmallRng;
use rand::seq::IteratorRandom;
use rand::{Rng, RngCore, SeedableRng};
use tracing::{error, info, trace};

use malachitebft_config::mempool_load::{NonUniformLoadConfig, UniformLoadConfig};
use malachitebft_config::MempoolLoadType;

use crate::fifo_mempool::{MempoolMsg, MempoolRef};
use crate::types::transaction::{Transaction, TransactionBatch};

pub type MempoolLoadMsg = Msg;
pub type MempoolLoadRef = ActorRef<Msg>;

#[derive(Clone, Debug, PartialEq)]
pub enum TestCheckTxOutcome {
    Success(TxHash),
    Error(TxHash, String),
}

impl CheckTxOutcome for TestCheckTxOutcome {
    fn is_valid(&self) -> bool {
        matches!(self, TestCheckTxOutcome::Success(_))
    }
    fn hash(&self) -> TxHash {
        match self {
            TestCheckTxOutcome::Success(hash) => hash.clone(),
            TestCheckTxOutcome::Error(hash, _) => hash.clone(),
        }
    }
}

pub enum Msg {
    GenerateTransactions { count: usize, size: ByteSize },
    AddTxReply(Transaction, TestCheckTxOutcome),
    Error(String),
}

#[derive(Debug)]
pub struct State {
    ticker: JoinHandle<()>,
}

#[derive(Debug, Default)]
pub struct Params {
    pub load_type: MempoolLoadType,
}

pub struct MempoolLoad {
    params: Params,
    mempool: MempoolRef,
    span: tracing::Span,
}

impl MempoolLoad {
    pub fn new(params: Params, mempool: MempoolRef, span: tracing::Span) -> Self {
        Self {
            params,
            mempool,
            span,
        }
    }

    pub async fn spawn(
        params: Params,
        mempool: MempoolRef,
        span: tracing::Span,
    ) -> Result<MempoolLoadRef, ractor::SpawnErr> {
        let actor = Self::new(params, mempool, span);
        let (actor_ref, _) = Actor::spawn(None, actor, ()).await?;
        Ok(actor_ref)
    }

    pub fn generate_transactions(count: usize, size: ByteSize) -> Vec<Transaction> {
        let mut transactions: Vec<Transaction> = Vec::with_capacity(count);
        let mut rng = SmallRng::from_entropy();

        for _ in 0..count {
            let mut tx_bytes = vec![0; size.as_u64() as usize];
            rng.fill_bytes(&mut tx_bytes);
            let tx = Transaction::new(tx_bytes);
            transactions.push(tx);
        }
        transactions
    }

    fn generate_non_uniform_load_params(
        params: &NonUniformLoadConfig,
    ) -> (usize, ByteSize, Duration) {
        let mut rng = SmallRng::from_entropy();

        // Determine if this iteration should generate a spike
        let is_spike = rng.gen_bool(params.spike_probability);

        // Vary transaction count and size
        let count_variation = rng.gen_range(params.count_variation.clone());
        let size_variation = rng.gen_range(params.size_variation.clone());

        let count = if is_spike {
            (params.base_count + count_variation) as usize * params.spike_multiplier
        } else {
            (params.base_count + count_variation) as usize
        };
        let size = (params.base_size + size_variation) as u64;

        // Get sleep duration
        let sleep_duration =
            Duration::from_millis(params.sleep_interval.clone().choose(&mut rng).unwrap());

        (count.max(1), ByteSize::b(size.max(1)), sleep_duration)
    }

    async fn run_uniform_load(params: UniformLoadConfig, myself: MempoolLoadRef) {
        loop {
            // Create and send the message
            let msg = Msg::GenerateTransactions {
                count: params.count,
                size: params.size,
            };

            if let Err(er) = myself.cast(msg) {
                tracing::error!(?er, ?myself, "Channel closed, stopping load generator");
                break;
            }

            tokio::time::sleep(params.interval).await;
        }
    }

    async fn run_non_uniform_load(params: NonUniformLoadConfig, myself: MempoolLoadRef) {
        loop {
            let (count, size, sleep_duration) = Self::generate_non_uniform_load_params(&params);

            // Create and send the message
            let msg = Msg::GenerateTransactions { count, size };

            if let Err(er) = myself.cast(msg) {
                tracing::error!(?er, ?myself, "Channel closed, stopping load generator");
                break;
            }

            tokio::time::sleep(sleep_duration).await;
        }
    }
}

#[async_trait]
impl Actor for MempoolLoad {
    type Msg = Msg;
    type State = State;
    type Arguments = ();

    async fn pre_start(
        &self,
        myself: MempoolLoadRef,
        _args: (),
    ) -> Result<State, ActorProcessingErr> {
        let ticker = match self.params.load_type.clone() {
            MempoolLoadType::NoLoad => tokio::spawn(async {}),
            MempoolLoadType::UniformLoad(uniform_load_config) => {
                tokio::spawn(Self::run_uniform_load(uniform_load_config, myself.clone()))
            }
            MempoolLoadType::NonUniformLoad(non_uniform_load_config) => tokio::spawn(
                Self::run_non_uniform_load(non_uniform_load_config, myself.clone()),
            ),
        };
        Ok(State { ticker })
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        info!("Stopping...");
        state.ticker.abort();
        Ok(())
    }

    #[tracing::instrument("host.mempool_load", parent = &self.span, skip_all)]
    async fn handle(
        &self,
        myself: MempoolLoadRef,
        msg: Msg,
        _state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            Msg::GenerateTransactions { count, size } => {
                let transactions = Self::generate_transactions(count, size);
                let tx_batch = TransactionBatch::new(transactions);

                for tx in tx_batch.into_vec().into_iter() {
                    let raw_tx = ::mempool::RawTx(tx.to_bytes());

                    self.mempool
                        .call_and_forward(
                            |reply| MempoolMsg::Add { tx: raw_tx, reply },
                            &myself,
                            move |outcome| match outcome {
                                Ok(check_tx_outcome) => {
                                    let tx_hash = check_tx_outcome.hash();
                                    if check_tx_outcome.is_valid() {
                                        Msg::AddTxReply(tx, TestCheckTxOutcome::Success(tx_hash))
                                    } else {
                                        Msg::AddTxReply(
                                            tx,
                                            TestCheckTxOutcome::Error(
                                                tx_hash,
                                                "Transaction validation failed".to_string(),
                                            ),
                                        )
                                    }
                                }
                                Err(e) => Msg::Error(e.to_string()),
                            },
                            None,
                        )
                        .map_err(|e| eyre!("Error when sending decided value to host: {e:?}"))?
                        .await
                        .map_err(|e| eyre!("Error waiting for result: {e:?}"))?;
                }
            }
            Msg::AddTxReply(_tx, outcome) => {
                trace!(tx_hash = ?outcome.hash(), is_valid = outcome.is_valid(), "Received transaction reply");
            }
            Msg::Error(e) => {
                error!(e, "Error:")
            }
        }
        Ok(())
    }
}
