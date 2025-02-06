use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use bytesize::ByteSize;
use malachitebft_starknet_host::node::StarknetNode;
use malachitebft_starknet_host::types::{Height, MockContext, PrivateKey, Validator, ValidatorSet};
use rand::rngs::StdRng;
use rand::SeedableRng;

use malachitebft_config::*;
use malachitebft_test_framework::HasTestRunner;
use malachitebft_test_framework::{NodeRunner, TestNode};

pub use malachitebft_test_framework::TestBuilder as GenTestBuilder;
pub use malachitebft_test_framework::{
    init_logging, EngineHandle, HandlerResult, Handles, Node, NodeId, TestParams,
};

use tempfile::TempDir;

#[cfg(test)]
pub mod tests;

pub type TestBuilder<S> = GenTestBuilder<MockContext, S>;

impl HasTestRunner<TestRunner> for MockContext {
    type Runner = TestRunner;
}

#[derive(Clone)]
pub struct TestRunner {
    pub id: usize,
    pub params: TestParams,
    pub nodes_count: usize,
    pub start_height: HashMap<NodeId, Height>,
    pub home_dir: HashMap<NodeId, PathBuf>,
    pub private_keys: HashMap<NodeId, PrivateKey>,
    pub validator_set: ValidatorSet,
    pub consensus_base_port: usize,
    pub mempool_base_port: usize,
    pub metrics_base_port: usize,
}

fn temp_dir(id: NodeId) -> PathBuf {
    TempDir::with_prefix(format!("malachitebft-test-app-{id}-"))
        .unwrap()
        .into_path()
}

#[async_trait]
impl NodeRunner<MockContext> for TestRunner {
    fn new<S>(id: usize, nodes: &[TestNode<MockContext, S>], params: TestParams) -> Self {
        let nodes_count = nodes.len();
        let base_port = 20_000 + id * 1000;

        let (validators, private_keys) = make_validators(nodes);
        let validator_set = ValidatorSet::new(validators);

        let start_height = nodes
            .iter()
            .map(|node| (node.id, node.start_height))
            .collect();

        let home_dir = nodes
            .iter()
            .map(|node| (node.id, temp_dir(node.id)))
            .collect();

        Self {
            id,
            params,
            nodes_count,
            start_height,
            home_dir,
            private_keys,
            validator_set,
            consensus_base_port: base_port,
            mempool_base_port: base_port + 100,
            metrics_base_port: base_port + 200,
        }
    }

    async fn spawn(&self, id: NodeId) -> eyre::Result<Handles<MockContext>> {
        let home_dir = &self.home_dir[&id].clone();

        let app = StarknetNode {
            config: self.generate_config(id),
            home_dir: home_dir.clone(),
            start_height: Some(self.start_height[&id].as_u64()),
        };

        let validators = self
            .validator_set
            .validators
            .iter()
            .map(|val| (val.public_key, val.voting_power))
            .collect();

        let genesis = app.make_genesis(validators);
        fs::create_dir_all(app.genesis_file().parent().unwrap())?;
        fs::write(app.genesis_file(), serde_json::to_string(&genesis)?)?;

        let priv_key_file = app.make_private_key_file(self.private_keys[&id]);
        fs::create_dir_all(app.private_key_file().parent().unwrap())?;
        fs::write(
            app.private_key_file(),
            serde_json::to_string(&priv_key_file)?,
        )?;

        app.start().await
    }

    async fn reset_db(&self, id: NodeId) -> eyre::Result<()> {
        let db_dir = self.home_dir[&id].join("db");
        fs::remove_dir_all(&db_dir)?;
        fs::create_dir_all(&db_dir)?;
        Ok(())
    }
}

impl TestRunner {
    fn generate_config(&self, node: NodeId) -> Config {
        let mut config = self.generate_default_config(node);
        self.params.apply_to_config(&mut config);
        config
    }

    fn generate_default_config(&self, node: NodeId) -> Config {
        let transport = transport_from_env(TransportProtocol::Tcp);
        let protocol = PubSubProtocol::default();
        let i = node - 1;

        Config {
            moniker: format!("node-{}", node),
            logging: LoggingConfig::default(),
            consensus: ConsensusConfig {
                max_block_size: ByteSize::mib(1),
                value_payload: ValuePayload::PartsOnly,
                timeouts: TimeoutConfig::default(),
                p2p: P2pConfig {
                    transport,
                    protocol,
                    discovery: DiscoveryConfig::default(),
                    listen_addr: transport.multiaddr("127.0.0.1", self.consensus_base_port + i),
                    persistent_peers: (0..self.nodes_count)
                        .filter(|j| i != *j)
                        .map(|j| transport.multiaddr("127.0.0.1", self.consensus_base_port + j))
                        .collect(),
                    ..Default::default()
                },
            },
            mempool: MempoolConfig {
                p2p: P2pConfig {
                    transport,
                    protocol,
                    listen_addr: transport.multiaddr("127.0.0.1", self.mempool_base_port + i),
                    persistent_peers: (0..self.nodes_count)
                        .filter(|j| i != *j)
                        .map(|j| transport.multiaddr("127.0.0.1", self.mempool_base_port + j))
                        .collect(),
                    ..Default::default()
                },
                max_tx_count: 10000,
                gossip_batch_size: 100,
            },
            sync: SyncConfig {
                enabled: true,
                status_update_interval: Duration::from_secs(2),
                request_timeout: Duration::from_secs(5),
            },
            metrics: MetricsConfig {
                enabled: false,
                listen_addr: format!("127.0.0.1:{}", self.metrics_base_port + i)
                    .parse()
                    .unwrap(),
            },
            runtime: RuntimeConfig::single_threaded(),
            test: TestConfig::default(),
        }
    }
}

fn transport_from_env(default: TransportProtocol) -> TransportProtocol {
    if let Ok(protocol) = std::env::var("MALACHITE_TRANSPORT") {
        TransportProtocol::from_str(&protocol).unwrap_or(default)
    } else {
        default
    }
}

fn make_validators<S>(
    nodes: &[TestNode<MockContext, S>],
) -> (Vec<Validator>, HashMap<NodeId, PrivateKey>) {
    let mut rng = StdRng::seed_from_u64(0x42);

    let mut validators = Vec::new();
    let mut private_keys = HashMap::new();

    for node in nodes {
        let sk = PrivateKey::generate(&mut rng);
        let val = Validator::new(sk.public_key(), node.voting_power);

        private_keys.insert(node.id, sk);

        if node.voting_power > 0 {
            validators.push(val);
        }
    }

    (validators, private_keys)
}
