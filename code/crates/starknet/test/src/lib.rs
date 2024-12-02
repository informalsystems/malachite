use core::fmt;
use std::fs::{create_dir_all, remove_dir_all};
use std::net::SocketAddr;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rand::rngs::StdRng;
use rand::SeedableRng;
use tokio::task::JoinSet;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, error_span, info, warn, Instrument};

use malachite_actors::util::events::{Event, RxEvent, TxEvent};
use malachite_common::VotingPower;
use malachite_config::{
    BlockSyncConfig, Config as NodeConfig, Config, LoggingConfig, PubSubProtocol, TestConfig,
    TransportProtocol,
};
use malachite_starknet_host::spawn::spawn_node_actor;
use malachite_starknet_host::types::MockContext;
use malachite_starknet_host::types::{Height, PrivateKey, Validator, ValidatorSet};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Expected {
    Exactly(usize),
    AtLeast(usize),
    AtMost(usize),
    LessThan(usize),
    GreaterThan(usize),
}

impl Expected {
    pub fn check(&self, actual: usize) -> bool {
        match self {
            Expected::Exactly(expected) => actual == *expected,
            Expected::AtLeast(expected) => actual >= *expected,
            Expected::AtMost(expected) => actual <= *expected,
            Expected::LessThan(expected) => actual < *expected,
            Expected::GreaterThan(expected) => actual > *expected,
        }
    }
}

impl fmt::Display for Expected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expected::Exactly(n) => write!(f, "exactly {n}"),
            Expected::AtLeast(n) => write!(f, "at least {n}"),
            Expected::AtMost(n) => write!(f, "at most {n}"),
            Expected::LessThan(n) => write!(f, "less than {n}"),
            Expected::GreaterThan(n) => write!(f, "greater than {n}"),
        }
    }
}

pub struct TestParams {
    pub enable_blocksync: bool,
    pub protocol: PubSubProtocol,
    pub block_size: ByteSize,
    pub tx_size: ByteSize,
    pub txs_per_part: usize,
    pub vote_extensions: Option<ByteSize>,
    pub value_payload: ValuePayload,
    pub max_retain_blocks: usize,
}

impl Default for TestParams {
    fn default() -> Self {
        Self {
            enable_blocksync: false,
            protocol: PubSubProtocol::default(),
            block_size: ByteSize::mib(1),
            tx_size: ByteSize::kib(1),
            txs_per_part: 256,
            vote_extensions: None,
            value_payload: ValuePayload::default(),
            max_retain_blocks: 50,
        }
    }
}

impl TestParams {
    fn apply_to_config(&self, config: &mut Config) {
        config.blocksync.enabled = self.enable_blocksync;
        config.consensus.p2p.protocol = self.protocol;
        config.consensus.max_block_size = self.block_size;
        config.consensus.value_payload = self.value_payload;
        config.test.tx_size = self.tx_size;
        config.test.txs_per_part = self.txs_per_part;
        config.test.vote_extensions.enabled = self.vote_extensions.is_some();
        config.test.vote_extensions.size = self.vote_extensions.unwrap_or_default();
        config.test.max_retain_blocks = self.max_retain_blocks;
    }
}

pub enum Step<S> {
    Crash(Duration),
    ResetDb,
    Restart(Duration),
    WaitUntil(u64),
    OnEvent(EventHandler<S>),
    Expect(Expected),
    Success,
    Fail(String),
}

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub type EventHandler<S> =
    Box<dyn Fn(Event<MockContext>, &mut S) -> Result<ControlFlow<()>, BoxError> + Send + Sync>;

pub type NodeId = usize;

pub struct TestNode<S> {
    pub id: NodeId,
    pub voting_power: VotingPower,
    pub start_height: Height,
    pub start_delay: Duration,
    pub steps: Vec<Step<S>>,
}

impl<S> TestNode<S> {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            voting_power: 1,
            start_height: Height::new(1, 1),
            start_delay: Duration::from_secs(0),
            steps: vec![],
        }
    }

    pub fn vp(mut self, power: VotingPower) -> Self {
        self.voting_power = power;
        self
    }

    pub fn start(self) -> Self {
        self.start_at(1)
    }

    pub fn start_at(self, height: u64) -> Self {
        self.start_after(height, Duration::from_secs(0))
    }

    pub fn start_after(mut self, height: u64, delay: Duration) -> Self {
        self.start_height.block_number = height;
        self.start_delay = delay;
        self
    }

    pub fn crash(mut self) -> Self {
        self.steps.push(Step::Crash(Duration::from_secs(0)));
        self
    }

    pub fn crash_after(mut self, duration: Duration) -> Self {
        self.steps.push(Step::Crash(duration));
        self
    }

    pub fn reset_db(mut self) -> Self {
        self.steps.push(Step::ResetDb);
        self
    }

    pub fn restart_after(mut self, delay: Duration) -> Self {
        self.steps.push(Step::Restart(delay));
        self
    }

    pub fn wait_until(mut self, height: u64) -> Self {
        self.steps.push(Step::WaitUntil(height));
        self
    }

    pub fn expect(mut self, expected: Expected) -> Self {
        self.steps.push(Step::Expect(expected));
        self
    }

    pub fn success(mut self) -> Self {
        self.steps.push(Step::Success);
        self
    }

    // pub fn pause(mut self) -> Self {
    //     self.steps.push(Step::Pause);
    //     self
    // }
}

fn unique_id() -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static ID: AtomicUsize = AtomicUsize::new(1);
    ID.fetch_add(1, Ordering::SeqCst)
}

pub struct Test<const N: usize, S> {
    pub id: usize,
    pub nodes: [TestNode<S>; N],
    pub private_keys: [PrivateKey; N],
    pub validator_set: ValidatorSet,
    pub consensus_base_port: usize,
    pub mempool_base_port: usize,
    pub metrics_base_port: usize,
}

impl<const N: usize, S> Test<N, S> {
    pub fn new(nodes: [TestNode<S>; N]) -> Self {
        let vals_and_keys = make_validators(voting_powers(&nodes));
        let (validators, private_keys): (Vec<_>, Vec<_>) = vals_and_keys.into_iter().unzip();
        let private_keys = private_keys.try_into().expect("N private keys");
        let validator_set = ValidatorSet::new(validators);
        let id = unique_id();
        let base_port = 20_000 + id * 1000;

        Self {
            id,
            nodes,
            private_keys,
            validator_set,
            consensus_base_port: base_port,
            mempool_base_port: base_port + 100,
            metrics_base_port: base_port + 200,
        }
    }

    pub fn generate_default_configs(&self) -> [Config; N] {
        let configs: Vec<_> = (0..N).map(|i| make_node_config(self, i)).collect();
        configs.try_into().expect("N configs")
    }

    pub fn generate_custom_configs(&self, params: TestParams) -> [Config; N] {
        let mut configs = self.generate_default_configs();
        for config in &mut configs {
            params.apply_to_config(config);
        }
        configs
    }

    pub async fn run(self, state: S, timeout: Duration)
    where
        S: Clone + Send + Sync + 'static,
    {
        let configs = self.generate_default_configs();
        self.run_with_config(configs, state, timeout).await
    }

    pub async fn run_with_custom_config(self, state: S, timeout: Duration, params: TestParams)
    where
        S: Clone + Send + Sync + 'static,
    {
        let configs = self.generate_custom_configs(params);
        self.run_with_config(configs, state, timeout).await
    }

    pub async fn run_with_config(self, configs: [Config; N], state: S, timeout: Duration)
    where
        S: Clone + Send + Sync + 'static,
    {
        init_logging();

        let _span = error_span!("test", id = %self.id).entered();

        let mut set = JoinSet::new();

        for ((node, config), private_key) in self
            .nodes
            .into_iter()
            .zip(configs.into_iter())
            .zip(self.private_keys.into_iter())
        {
            let validator_set = self.validator_set.clone();

            let home_dir =
                tempfile::TempDir::with_prefix(format!("malachite-starknet-test-{}", self.id))
                    .unwrap()
                    .into_path();

            let state = state.clone();

            set.spawn(
                async move {
                    let id = node.id;
                    let result =
                        run_node(node, home_dir, config, validator_set, private_key, state).await;
                    (id, result)
                }
                .in_current_span(),
            );
        }

        let metrics = tokio::spawn(serve_metrics("127.0.0.1:0".parse().unwrap()));
        let results = tokio::time::timeout(timeout, set.join_all()).await;
        metrics.abort();

        match results {
            Ok(results) => {
                check_results(results);
            }
            Err(_) => {
                error!("Test timed out after {timeout:?}");
                std::process::exit(1);
            }
        }
    }
}

fn check_results(results: Vec<(NodeId, TestResult)>) {
    let mut errors = 0;

    for (id, result) in results {
        let _span = tracing::error_span!("node", %id).entered();
        match result {
            TestResult::Success(reason) => {
                info!("Test succeeded: {reason}");
            }
            TestResult::Failure(reason) => {
                errors += 1;
                error!("Test failed: {reason}");
            }
            TestResult::Unknown => {
                errors += 1;
                warn!("Test result is unknown");
            }
        }
    }

    if errors > 0 {
        error!("Test failed with {errors} errors");
        std::process::exit(1);
    }
}

pub enum TestResult {
    Success(String),
    Failure(String),
    Unknown,
}

#[tracing::instrument("node", skip_all, fields(id = %node.id))]
async fn run_node<S>(
    node: TestNode<S>,
    home_dir: PathBuf,
    config: Config,
    validator_set: ValidatorSet,
    private_key: PrivateKey,
    mut state: S,
) -> TestResult {
    sleep(node.start_delay).await;

    info!("Spawning node with voting power {}", node.voting_power);

    let tx_event = TxEvent::new();
    let mut rx_event = tx_event.subscribe();
    let rx_event_bg = tx_event.subscribe();

    let (mut actor_ref, mut handle) = spawn_node_actor(
        config.clone(),
        home_dir.clone(),
        validator_set.clone(),
        private_key,
        Some(node.start_height),
        tx_event,
    )
    .await;

    let decisions = Arc::new(AtomicUsize::new(0));

    let spawn_bg = |mut rx: RxEvent<MockContext>| {
        tokio::spawn({
            let decisions = Arc::clone(&decisions);

            async move {
                while let Ok(event) = rx.recv().await {
                    if let Event::Decided(_) = &event {
                        decisions.fetch_add(1, Ordering::SeqCst);
                    }

                    debug!("Event: {event:?}");
                }
            }
            .in_current_span()
        })
    };

    let mut bg = spawn_bg(rx_event_bg);

    for step in node.steps {
        match step {
            Step::WaitUntil(target_height) => {
                info!("Waiting until node reaches height {target_height}");

                'inner: while let Ok(event) = rx_event.recv().await {
                    let Event::Decided(decision) = event else {
                        continue;
                    };

                    let height = decision.height.as_u64();
                    info!("Node reached height {height}");

                    if height == target_height {
                        sleep(Duration::from_millis(100)).await;
                        break 'inner;
                    }
                }
            }

            Step::Crash(after) => {
                let height = decisions.load(Ordering::SeqCst);
                info!("Node crashes at height {height} after {after:?}");

                sleep(after).await;

                actor_ref.kill_and_wait(None).await.expect("Node must stop");
            }

            Step::ResetDb => {
                info!("Resetting database");

                let db_path = home_dir.join("db");
                remove_dir_all(&db_path).expect("Database must be removed");
                create_dir_all(&db_path).expect("Database must be created");
            }

            Step::Restart(after) => {
                info!("Node will restart in {after:?}");

                sleep(after).await;

                bg.abort();
                handle.abort();

                let tx_event = TxEvent::new();
                let new_rx_event = tx_event.subscribe();
                let new_rx_event_bg = tx_event.subscribe();

                let (new_actor_ref, new_handle) = spawn_node_actor(
                    config.clone(),
                    home_dir.clone(),
                    validator_set.clone(),
                    private_key,
                    Some(node.start_height),
                    tx_event,
                )
                .await;

                bg = spawn_bg(new_rx_event_bg);

                actor_ref = new_actor_ref;
                handle = new_handle;
                rx_event = new_rx_event;
            }

            Step::OnEvent(on_event) => {
                'inner: while let Ok(event) = rx_event.recv().await {
                    match on_event(event, &mut state) {
                        Ok(ControlFlow::Continue(_)) => {
                            continue 'inner;
                        }
                        Ok(ControlFlow::Break(_)) => {
                            break 'inner;
                        }
                        Err(e) => {
                            actor_ref.stop(Some("Test failed".to_string()));
                            handle.abort();
                            bg.abort();

                            return TestResult::Failure(e.to_string());
                        }
                    }
                }
            }

            Step::Expect(expected) => {
                let actual = decisions.load(Ordering::SeqCst);

                actor_ref.stop(Some("Test is over".to_string()));
                handle.abort();
                bg.abort();

                if expected.check(actual) {
                    return TestResult::Success(format!(
                        "Correct number of decisions: got {actual}, expected: {expected}"
                    ));
                } else {
                    return TestResult::Failure(format!(
                        "Incorrect number of decisions: got {actual}, expected: {expected}"
                    ));
                }
            }

            Step::Success => {
                actor_ref.stop(Some("Test succeeded".to_string()));
                handle.abort();
                bg.abort();

                return TestResult::Success("OK".to_string());
            }

            Step::Fail(reason) => {
                actor_ref.stop(Some("Test failed".to_string()));
                handle.abort();
                bg.abort();

                return TestResult::Failure(reason);
            }
        }
    }

    actor_ref.stop(Some("Test is over".to_string()));
    handle.abort();
    bg.abort();

    return TestResult::Unknown;
}

fn init_logging() {
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{EnvFilter, FmtSubscriber};

    let directive = if matches!(std::env::var("TEST_DEBUG").as_deref(), Ok("1")) {
        "malachite=debug,malachite_starknet_test=debug,ractor=error"
    } else {
        "malachite=error,malachite_starknet_test=debug,ractor=error"
    };

    let filter = EnvFilter::builder().parse(directive).unwrap();

    pub fn enable_ansi() -> bool {
        use std::io::IsTerminal;
        std::io::stdout().is_terminal() && std::io::stderr().is_terminal()
    }

    // Construct a tracing subscriber with the supplied filter and enable reloading.
    let builder = FmtSubscriber::builder()
        .with_target(false)
        .with_env_filter(filter)
        .with_writer(std::io::stdout)
        .with_ansi(enable_ansi())
        .with_thread_ids(false);

    let subscriber = builder.finish();

    if let Err(e) = subscriber.try_init() {
        eprintln!("Failed to initialize logging: {e}");
    }
}

use bytesize::ByteSize;

use malachite_config::{
    ConsensusConfig, MempoolConfig, MetricsConfig, P2pConfig, RuntimeConfig, TimeoutConfig,
    ValuePayload,
};

fn transport_from_env(default: TransportProtocol) -> TransportProtocol {
    if let Ok(protocol) = std::env::var("MALACHITE_TRANSPORT") {
        TransportProtocol::from_str(&protocol).unwrap_or(default)
    } else {
        default
    }
}

pub fn make_node_config<const N: usize, S>(test: &Test<N, S>, i: usize) -> NodeConfig {
    let transport = transport_from_env(TransportProtocol::Tcp);
    let protocol = PubSubProtocol::default();

    NodeConfig {
        moniker: format!("node-{}", test.nodes[i].id),
        logging: LoggingConfig::default(),
        consensus: ConsensusConfig {
            max_block_size: ByteSize::mib(1),
            value_payload: ValuePayload::default(),
            timeouts: TimeoutConfig::default(),
            p2p: P2pConfig {
                transport,
                protocol,
                listen_addr: transport.multiaddr("127.0.0.1", test.consensus_base_port + i),
                persistent_peers: (0..N)
                    .filter(|j| i != *j)
                    .map(|j| transport.multiaddr("127.0.0.1", test.consensus_base_port + j))
                    .collect(),
                ..Default::default()
            },
        },
        mempool: MempoolConfig {
            p2p: P2pConfig {
                transport,
                protocol,
                listen_addr: transport.multiaddr("127.0.0.1", test.mempool_base_port + i),
                persistent_peers: (0..N)
                    .filter(|j| i != *j)
                    .map(|j| transport.multiaddr("127.0.0.1", test.mempool_base_port + j))
                    .collect(),
                ..Default::default()
            },
            max_tx_count: 10000,
            gossip_batch_size: 100,
        },
        blocksync: BlockSyncConfig {
            enabled: true,
            status_update_interval: Duration::from_secs(2),
            request_timeout: Duration::from_secs(5),
        },
        metrics: MetricsConfig {
            enabled: false,
            listen_addr: format!("127.0.0.1:{}", test.metrics_base_port + i)
                .parse()
                .unwrap(),
        },
        runtime: RuntimeConfig::single_threaded(),
        test: TestConfig::default(),
    }
}

fn voting_powers<const N: usize, S>(nodes: &[TestNode<S>; N]) -> [VotingPower; N] {
    let mut voting_powers = [0; N];
    for (i, node) in nodes.iter().enumerate() {
        voting_powers[i] = node.voting_power;
    }
    voting_powers
}

pub fn make_validators<const N: usize>(
    voting_powers: [VotingPower; N],
) -> [(Validator, PrivateKey); N] {
    let mut rng = StdRng::seed_from_u64(0x42);

    let mut validators = Vec::with_capacity(N);

    for vp in voting_powers {
        let sk = PrivateKey::generate(&mut rng);
        let val = Validator::new(sk.public_key(), vp);
        validators.push((val, sk));
    }

    validators.try_into().expect("N validators")
}

use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;

#[tracing::instrument(name = "metrics", skip_all)]
async fn serve_metrics(listen_addr: SocketAddr) {
    let app = Router::new().route("/metrics", get(get_metrics));
    let listener = TcpListener::bind(listen_addr).await.unwrap();
    let address = listener.local_addr().unwrap();

    async fn get_metrics() -> String {
        let mut buf = String::new();
        malachite_metrics::export(&mut buf);
        buf
    }

    info!(%address, "Serving metrics");
    axum::serve(listener, app).await.unwrap();
}
