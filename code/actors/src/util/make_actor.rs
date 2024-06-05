use std::time::Duration;

use ractor::ActorRef;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use malachite_common::Round;
use malachite_gossip_consensus::Keypair;
use malachite_gossip_mempool::Config as GossipMempoolConfig;
use malachite_node::config::Config as NodeConfig;
use malachite_test::{Address, Height, PrivateKey, TestContext, ValidatorSet, Value};

use crate::consensus::Consensus;
use crate::gossip::Gossip;
use crate::gossip_mempool::GossipMempool;
use crate::host::Host;
use crate::mempool::Mempool;
use crate::node::{Msg as NodeMsg, Msg, Node};
use crate::util::value_builder::test::TestParams as TestValueBuilderParams;
use crate::util::PartStore;
use crate::util::TestValueBuilder;

pub async fn make_node_actor(
    cfg: NodeConfig,
    initial_validator_set: ValidatorSet,
    validator_pk: PrivateKey,
    node_pk: PrivateKey,
    address: Address,
    tx_decision: mpsc::Sender<(Height, Round, Value)>,
) -> (ActorRef<NodeMsg>, JoinHandle<()>) {
    let ctx = TestContext::new(validator_pk.clone());

    // Spawn mempool and its gossip
    let config_gossip_mempool = GossipMempoolConfig {
        listen_addr: cfg.mempool.p2p.listen_addr.clone(),
        persistent_peers: cfg.mempool.p2p.persistent_peers.clone(),
        idle_connection_timeout: Duration::from_secs(60),
    };

    let node_keypair = Keypair::ed25519_from_bytes(node_pk.inner().to_bytes()).unwrap();

    let gossip_mempool = GossipMempool::spawn(node_keypair.clone(), config_gossip_mempool, None)
        .await
        .unwrap();

    let mempool = Mempool::spawn(gossip_mempool.clone(), None).await.unwrap();

    // Configure the value builder
    let value_builder = Box::new(TestValueBuilder::<TestContext>::new(
        mempool.clone(),
        TestValueBuilderParams {
            max_block_size: cfg.consensus.max_block_size,
            txs_per_part: cfg.test.txs_per_part,
            time_allowance_factor: cfg.test.time_allowance_factor,
            exec_time_per_part: cfg.test.exec_time_per_part,
        },
    ));

    // Spawn the host actor
    let host = Host::spawn(
        value_builder,
        PartStore::new(),
        initial_validator_set.clone(),
    )
    .await
    .unwrap();

    // Spawn consensus and its gossip
    let config_gossip = malachite_gossip_consensus::Config {
        listen_addr: cfg.consensus.p2p.listen_addr.clone(),
        persistent_peers: cfg.consensus.p2p.persistent_peers.clone(),
        idle_connection_timeout: Duration::from_secs(60),
    };

    let validator_keypair = Keypair::ed25519_from_bytes(validator_pk.inner().to_bytes()).unwrap();

    let gossip_consensus = Gossip::spawn(validator_keypair.clone(), config_gossip, None)
        .await
        .unwrap();

    let start_height = Height::new(1);

    let consensus_params = crate::consensus::Params {
        start_height,
        initial_validator_set,
        address,
        threshold_params: Default::default(),
    };

    let consensus = Consensus::spawn(
        ctx.clone(),
        consensus_params,
        cfg.consensus.timeouts,
        gossip_consensus.clone(),
        host.clone(),
        tx_decision,
        None,
    )
    .await
    .unwrap();

    // Spawn the node actor
    let node = Node::new(
        ctx,
        gossip_consensus,
        consensus,
        gossip_mempool,
        mempool,
        host,
        start_height,
    );

    let (actor_ref, handle) = node.spawn().await.unwrap();
    actor_ref.cast(Msg::Start).unwrap();

    (actor_ref, handle)
}
