use ractor::ActorRef;
use tokio::sync::mpsc;

use malachite_common::Round;
use malachite_gossip::{Keypair, PeerId};

use crate::gossip_mempool::GossipMempool;
use crate::mempool::Mempool;
use malachite_gossip_mempool::Multiaddr;
use malachite_test::{Address, Height, PrivateKey, TestContext, ValidatorSet, Value};
use tokio::task::JoinHandle;

use crate::node::{Msg as NodeMsg, Params as NodeParams};
use crate::timers::Config as TimersConfig;
use crate::util::TestValueBuilder;

pub async fn make_node_actor(
    initial_validator_set: ValidatorSet,
    validator_pks: Vec<PrivateKey>,
    validator_pk: PrivateKey,
    nodes_pks: Vec<PrivateKey>,
    node_pk: PrivateKey,
    address: Address,
    tx_decision: mpsc::Sender<(Height, Round, Value)>,
) -> (ActorRef<NodeMsg>, JoinHandle<()>) {
    // let addr: Multiaddr = "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap();
    let addr: Multiaddr = "/ip4/0.0.0.0/tcp/0".parse().unwrap();
    let config_mempool = malachite_gossip_mempool::Config::default();

    let node_keypair = Keypair::ed25519_from_bytes(node_pk.inner().to_bytes()).unwrap();

    let node_keypairs: Vec<Keypair> = nodes_pks
        .iter()
        .map(|pk| Keypair::ed25519_from_bytes(pk.inner().to_bytes()).unwrap())
        .collect();
    let node_peer_ids = node_keypairs
        .iter()
        .map(|pk| PeerId::from_public_key(&pk.public()))
        .collect();

    let gossip_mempool = GossipMempool::spawn(
        node_keypair.clone(),
        addr,
        node_peer_ids,
        config_mempool,
        None,
    )
    .await
    .unwrap();

    let mempool = Mempool::spawn(crate::mempool::Params {}, gossip_mempool.clone(), None)
        .await
        .unwrap();

    let builder = TestValueBuilder::<TestContext>::new(mempool.clone());
    let value_builder = Box::new(builder);

    let validator_keypair = Keypair::ed25519_from_bytes(validator_pk.inner().to_bytes()).unwrap();

    let validator_keypairs: Vec<Keypair> = validator_pks
        .iter()
        .map(|pk| Keypair::ed25519_from_bytes(pk.inner().to_bytes()).unwrap())
        .collect();
    let validator_peer_ids = validator_keypairs
        .iter()
        .map(|pk| PeerId::from_public_key(&pk.public()))
        .collect();

    let start_height = Height::new(1);
    let ctx = TestContext::new(validator_pk.clone());

    let timers_config = TimersConfig::default();

    let params = NodeParams {
        address,
        initial_validator_set,
        validator_peer_ids,
        keypair: validator_keypair.clone(),
        start_height,
        threshold_params: Default::default(),
        timers_config,
        tx_decision,
        value_builder,
        gossip_mempool,
        mempool,
    };

    crate::node::spawn(ctx, params).await.unwrap()
}
