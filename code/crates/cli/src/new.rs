//! key and configuration generation

use bytesize::ByteSize;
use itertools::Itertools;
use rand::prelude::StdRng;
use rand::rngs::OsRng;
use rand::{seq::IteratorRandom, Rng, SeedableRng};

use malachite_common::{PrivateKey, PublicKey};
use malachite_config::*;
use malachite_node::Node;

const MIN_VOTING_POWER: u64 = 1;
const MAX_VOTING_POWER: u64 = 1;

const CONSENSUS_BASE_PORT: usize = 27000;
const MEMPOOL_BASE_PORT: usize = 28000;
const METRICS_BASE_PORT: usize = 29000;

/// Generate private keys. Random or deterministic for different use-cases.
pub fn generate_private_keys<N>(
    node: &N,
    size: usize,
    deterministic: bool,
) -> Vec<PrivateKey<N::Context>>
where
    N: Node,
{
    if deterministic {
        let mut rng = StdRng::seed_from_u64(0x42);
        (0..size)
            .map(|_| node.generate_private_key(&mut rng))
            .collect()
    } else {
        (0..size)
            .map(|_| node.generate_private_key(OsRng))
            .collect()
    }
}

/// Generate a Genesis file from the public keys and voting power.
/// Voting power can be random or deterministically pseudo-random.
pub fn generate_genesis<N: Node>(
    node: &N,
    pks: Vec<PublicKey<N::Context>>,
    deterministic: bool,
) -> N::Genesis {
    let validators: Vec<_> = if deterministic {
        let mut rng = StdRng::seed_from_u64(0x42);
        pks.into_iter()
            .map(|pk| (pk, rng.gen_range(MIN_VOTING_POWER..=MAX_VOTING_POWER)))
            .collect()
    } else {
        pks.into_iter()
            .map(|pk| (pk, OsRng.gen_range(MIN_VOTING_POWER..=MAX_VOTING_POWER)))
            .collect()
    };

    node.make_genesis(validators)
}

/// Generate configuration for node "index" out of "total" number of nodes.
#[allow(clippy::too_many_arguments)]
pub fn generate_config(
    index: usize,
    total: usize,
    runtime: RuntimeConfig,
    enable_discovery: bool,
    transport: TransportProtocol,
    logging: LoggingConfig,
) -> Config {
    let consensus_port = CONSENSUS_BASE_PORT + index;
    let mempool_port = MEMPOOL_BASE_PORT + index;
    let metrics_port = METRICS_BASE_PORT + index;

    Config {
        moniker: format!("test-{}", index),
        consensus: ConsensusConfig {
            max_block_size: ByteSize::mib(1),
            timeouts: TimeoutConfig::default(),
            p2p: P2pConfig {
                protocol: PubSubProtocol::default(),
                listen_addr: transport.multiaddr("127.0.0.1", consensus_port),
                persistent_peers: if enable_discovery {
                    let mut rng = rand::thread_rng();
                    let count = if total > 1 {
                        rng.gen_range(1..=(total / 2))
                    } else {
                        0
                    };
                    let peers = (0..total)
                        .filter(|j| *j != index)
                        .choose_multiple(&mut rng, count);

                    peers
                        .iter()
                        .unique()
                        .map(|index| transport.multiaddr("127.0.0.1", CONSENSUS_BASE_PORT + index))
                        .collect()
                } else {
                    (0..total)
                        .filter(|j| *j != index)
                        .map(|j| transport.multiaddr("127.0.0.1", CONSENSUS_BASE_PORT + j))
                        .collect()
                },
                discovery: DiscoveryConfig {
                    enabled: enable_discovery,
                },
                transport,
            },
        },
        mempool: MempoolConfig {
            p2p: P2pConfig {
                protocol: PubSubProtocol::default(),
                listen_addr: transport.multiaddr("127.0.0.1", mempool_port),
                persistent_peers: (0..total)
                    .filter(|j| *j != index)
                    .map(|j| transport.multiaddr("127.0.0.1", MEMPOOL_BASE_PORT + j))
                    .collect(),
                discovery: DiscoveryConfig { enabled: true },
                transport,
            },
            max_tx_count: 10000,
            gossip_batch_size: 0,
        },
        blocksync: Default::default(),
        metrics: MetricsConfig {
            enabled: true,
            listen_addr: format!("127.0.0.1:{metrics_port}").parse().unwrap(),
        },
        logging,
        runtime,
        test: TestConfig::default(),
    }
}