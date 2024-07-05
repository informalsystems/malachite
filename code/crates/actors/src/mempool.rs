use std::collections::{BTreeMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use async_trait::async_trait;
use ractor::{Actor, ActorCell, ActorProcessingErr, ActorRef, RpcReplyPort};
use rand::distributions::Uniform;
use rand::Rng;
use tracing::{info, trace};

use malachite_common::{MempoolTransactionBatch, Transaction, TransactionBatch};
use malachite_gossip_mempool::{Channel, Event as GossipEvent, NetworkMsg, PeerId};
use malachite_node::config::{MempoolConfig, TestConfig};

use crate::gossip_mempool::{GossipMempoolRef, Msg as GossipMempoolMsg};
use crate::util::forward;

pub type MempoolRef = ActorRef<MempoolMsg>;

pub struct Mempool {
    gossip_mempool: GossipMempoolRef,
    mempool_config: MempoolConfig, // todo - pick only what's needed
    test_config: TestConfig,       // todo - pick only the mempool related
}

#[derive(Debug)]
pub enum MempoolMsg {
    GossipEvent(Arc<GossipEvent>),
    Input(Transaction),
    TxStream {
        height: u64,
        num_txes: usize,
        reply: RpcReplyPort<Vec<Transaction>>,
    },
    Update {
        tx_hashes: Vec<u64>,
    },
}

#[allow(dead_code)]
pub struct State {
    pub msg_queue: VecDeque<MempoolMsg>,
    pub transactions: BTreeMap<u64, Transaction>,
}

impl State {
    pub fn new() -> Self {
        Self {
            msg_queue: VecDeque::new(),
            transactions: BTreeMap::new(),
        }
    }

    pub fn add_tx(&mut self, tx: &Transaction) {
        let mut hash = DefaultHasher::new();
        tx.0.hash(&mut hash);
        let key = hash.finish();
        self.transactions.entry(key).or_insert(tx.clone());
    }

    pub fn remove_tx(&mut self, hash: &u64) {
        self.transactions.remove_entry(hash);
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl Mempool {
    pub fn new(
        gossip_mempool: GossipMempoolRef,
        mempool_config: MempoolConfig,
        test_config: TestConfig,
    ) -> Self {
        Self {
            gossip_mempool,
            mempool_config,
            test_config,
        }
    }

    pub async fn spawn(
        gossip_mempool: GossipMempoolRef,
        mempool_config: &MempoolConfig,
        test_config: &TestConfig,
        supervisor: Option<ActorCell>,
    ) -> Result<MempoolRef, ractor::SpawnErr> {
        let node = Self::new(gossip_mempool, mempool_config.clone(), *test_config);

        let (actor_ref, _) = if let Some(supervisor) = supervisor {
            Actor::spawn_linked(None, node, (), supervisor).await?
        } else {
            Actor::spawn(None, node, ()).await?
        };

        Ok(actor_ref)
    }

    pub async fn handle_gossip_event(
        &self,
        event: &GossipEvent,
        myself: MempoolRef,
        state: &mut State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        match event {
            GossipEvent::Listening(addr) => {
                info!("Listening on {addr}");
            }
            GossipEvent::PeerConnected(peer_id) => {
                info!("Connected to peer {peer_id}");
            }
            GossipEvent::PeerDisconnected(peer_id) => {
                info!("Disconnected from peer {peer_id}");
            }
            GossipEvent::Message(from, msg) => {
                // TODO: Implement Protobuf on NetworkMsg
                // trace!(%from, "Received message of size {} bytes", msg.encoded_len());
                trace!(%from, "Received message");
                self.handle_network_msg(from, msg.clone(), myself, state) // FIXME: Clone
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn handle_network_msg(
        &self,
        from: &PeerId,
        msg: NetworkMsg,
        myself: MempoolRef,
        _state: &mut State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        match msg {
            NetworkMsg::TransactionBatch(batch) => {
                trace!(%from, "Received batch with {} transactions", batch.len());

                for tx in batch.transaction_batch.into_transactions() {
                    myself.cast(MempoolMsg::Input(tx))?;
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Actor for Mempool {
    type Msg = MempoolMsg;
    type State = State;
    type Arguments = ();

    async fn pre_start(
        &self,
        myself: MempoolRef,
        _args: (),
    ) -> Result<State, ractor::ActorProcessingErr> {
        let forward = forward(
            myself.clone(),
            Some(myself.get_cell()),
            MempoolMsg::GossipEvent,
        )
        .await?;
        self.gossip_mempool
            .cast(GossipMempoolMsg::Subscribe(forward))?;

        Ok(State::new())
    }

    #[tracing::instrument(name = "mempool", skip(self, myself, msg, state))]
    async fn handle(
        &self,
        myself: MempoolRef,
        msg: MempoolMsg,
        state: &mut State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        match msg {
            MempoolMsg::GossipEvent(event) => {
                self.handle_gossip_event(&event, myself, state).await?;
            }

            MempoolMsg::Input(tx) => {
                if state.transactions.len() < self.mempool_config.max_tx_count {
                    state.add_tx(&tx);
                } else {
                    trace!("Mempool is full, dropping transaction");
                }
            }

            // Adi: This is a request coming from `build_new_proposal`
            MempoolMsg::TxStream {
                reply, num_txes, ..
            } => {
                let txes = generate_and_broadcast_txes(
                    num_txes,
                    self.test_config.tx_size.as_u64(),
                    &self.mempool_config,
                    state,
                    &self.gossip_mempool,
                )?;

                reply.send(txes)?;
            }

            MempoolMsg::Update { .. } => {
                // tx_hashes.iter().for_each(|hash| state.remove_tx(hash));

                // FIXME: Reset the mempool for now
                state.transactions.clear();
            }
        }

        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        _state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        info!("Stopping...");

        Ok(())
    }
}

fn generate_and_broadcast_txes(
    count: usize,
    size: u64,
    config: &MempoolConfig,
    state: &mut State,
    gossip_mempool: &GossipMempoolRef,
) -> Result<Vec<Transaction>, ActorProcessingErr> {
    let mut transactions = vec![];
    let mut tx_batch = TransactionBatch::default();
    let mut rng = rand::thread_rng();

    for _ in 0..count {
        // Generate transaction
        let range = Uniform::new(32, 64);
        let tx_bytes: Vec<u8> = (0..size).map(|_| rng.sample(range)).collect();
        let tx = Transaction::new(tx_bytes);

        info!("\t .. Generating transactions up to {count}");

        // Add transaction to state
        if state.transactions.len() < config.max_tx_count {
            state.add_tx(&tx);
            info!(
                "\t .. State now has len={} transactions",
                state.transactions.len()
            );
        }
        tx_batch.push(tx.clone());
        info!("\t .. tx_batch now has len={} transactions", tx_batch.len());

        // Gossip tx-es to peers in batches
        if config.gossip_batch_size > 0 && tx_batch.len() >= config.gossip_batch_size {
            // TODO(Adi): Isn't there a bug here --
            //  Shouldn't we reset `tx_batch` to an empty vector?
            //  Or is that done implicitly in `MempoolTransactionBatch::new`?
            let mempool_batch = MempoolTransactionBatch::new(std::mem::take(&mut tx_batch));
            info!(
                "\t .. assembled a mempool batch len={}; size={}",
                mempool_batch.len(),
                mempool_batch.transaction_batch.size_bytes()
            );

            // TODO(Adi): What happens with the broadcast message below? Could not trace it b/c
            //  it gets lost somewhere in a "gossip controller" indirection.
            gossip_mempool.cast(GossipMempoolMsg::Broadcast(Channel::Mempool, mempool_batch))?;
        }

        transactions.push(tx);
    }
    
    info!(
        "\t .. returning with transactions len={}",
        transactions.len()
    );

    Ok(transactions)
}
