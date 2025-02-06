use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::async_trait;
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::{debug, error, error_span, info, Instrument};

use malachitebft_core_types::{Context, Height};
use malachitebft_engine::util::events::{Event, RxEvent};

pub use malachitebft_app::{EngineHandle, Handles, Node};

mod logging;
pub use logging::init_logging;

mod node;
pub use node::{HandlerResult, NodeId, TestNode};

mod params;
pub use params::TestParams;

mod expected;
pub use expected::Expected;

use node::Step;

fn unique_id() -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static ID: AtomicUsize = AtomicUsize::new(1);
    ID.fetch_add(1, Ordering::SeqCst)
}

pub struct TestBuilder<Ctx, S>
where
    Ctx: Context,
{
    nodes: Vec<TestNode<Ctx, S>>,
}

impl<Ctx, S> Default for TestBuilder<Ctx, S>
where
    Ctx: Context,
{
    fn default() -> Self {
        Self { nodes: Vec::new() }
    }
}

impl<Ctx, S> TestBuilder<Ctx, S>
where
    Ctx: Context,
    S: Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self) -> &mut TestNode<Ctx, S>
    where
        S: Default,
    {
        let node = TestNode::new(self.nodes.len() + 1);
        self.nodes.push(node);
        self.nodes.last_mut().unwrap()
    }

    pub fn build(self) -> Test<Ctx, S> {
        Test::new(self.nodes)
    }
}

/// In order to work around orphan rules, `R` must be a type
/// defined in the same crate where this trait is implemented.
/// It does not matter what the type is, as long as it is local.
/// You can use the same type as for the `Runner` type member.
pub trait HasTestRunner<R>: Context {
    type Runner: NodeRunner<Self>;
}

pub struct Test<Ctx, S>
where
    Ctx: Context,
{
    pub id: usize,
    pub nodes: Vec<TestNode<Ctx, S>>,
}

impl<Ctx, S> Test<Ctx, S>
where
    Ctx: Context,
{
    pub fn new(nodes: Vec<TestNode<Ctx, S>>) -> Self {
        Self {
            id: unique_id(),
            nodes,
        }
    }

    pub async fn run<R>(self, timeout: Duration)
    where
        Ctx: HasTestRunner<R>,
        S: Send + Sync + 'static,
    {
        self.run_with_params(timeout, TestParams::default()).await
    }

    pub async fn run_with_params<R>(self, timeout: Duration, params: TestParams)
    where
        Ctx: HasTestRunner<R>,
        S: Send + Sync + 'static,
    {
        run_test::<Ctx::Runner, Ctx, S>(self, timeout, params).await
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
}

pub async fn run_test<R, Ctx, S>(test: Test<Ctx, S>, timeout: Duration, params: TestParams)
where
    Ctx: Context,
    R: NodeRunner<Ctx>,
    S: Send + Sync + 'static,
{
    let span = error_span!("test", id = %test.id);

    let mut set = JoinSet::new();

    let runner = R::new(test.id, &test.nodes, params);

    for node in test.nodes {
        let runner = runner.clone();

        set.spawn(
            async move {
                let id = node.id;
                let result = run_node(runner, node).await;
                (id, result)
            }
            .instrument(span.clone()),
        );
    }

    let results = tokio::time::timeout(timeout, set.join_all()).await;

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

#[async_trait]
pub trait NodeRunner<Ctx>
where
    Self: Clone + Send + Sync + 'static,
    Ctx: Context,
{
    fn new<S>(id: usize, nodes: &[TestNode<Ctx, S>], params: TestParams) -> Self;

    async fn spawn(&self, id: NodeId) -> eyre::Result<Handles<Ctx>>;
    async fn reset_db(&self, id: NodeId) -> eyre::Result<()>;
}

#[tracing::instrument("node", skip_all, fields(id = %node.id))]
pub async fn run_node<Ctx, R, S>(runner: R, mut node: TestNode<Ctx, S>) -> TestResult
where
    Ctx: Context,
    R: NodeRunner<Ctx>,
    S: Send + Sync + 'static,
{
    sleep(node.start_delay).await;

    info!(%node.voting_power, "Spawning node");

    let mut handles = runner.spawn(node.id).await.unwrap();

    let mut rx_event = handles.tx_event.subscribe();
    let rx_event_bg = handles.tx_event.subscribe();

    let decisions = Arc::new(AtomicUsize::new(0));
    let current_height = Arc::new(AtomicUsize::new(0));
    let is_full_node = node.is_full_node();

    let spawn_bg = |mut rx: RxEvent<Ctx>| {
        tokio::spawn({
            let decisions = Arc::clone(&decisions);
            let current_height = Arc::clone(&current_height);

            async move {
                while let Ok(event) = rx.recv().await {
                    match &event {
                        Event::StartedHeight(height) => {
                            current_height.store(height.as_u64() as usize, Ordering::SeqCst);
                        }
                        Event::Decided(_) => {
                            decisions.fetch_add(1, Ordering::SeqCst);
                        }
                        Event::Published(msg) if is_full_node => {
                            panic!(
                                "Full nodes unexpectedly published a consensus message: {msg:?}"
                            );
                        }
                        _ => (),
                    }

                    debug!("Event: {event}");
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
                    let Event::StartedHeight(height) = event else {
                        continue;
                    };

                    info!("Node started height {height}");

                    if height.as_u64() == target_height {
                        break 'inner;
                    }
                }
            }

            Step::Crash(after) => {
                let height = current_height.load(Ordering::SeqCst);

                info!("Node will crash at height {height}");
                sleep(after).await;

                handles
                    .engine
                    .actor
                    .kill_and_wait(None)
                    .await
                    .expect("Node must stop");

                bg.abort();
                handles.app.abort();
                handles.engine.handle.abort();
            }

            Step::ResetDb => {
                info!("Resetting database");
                runner.reset_db(node.id).await.unwrap();
            }

            Step::Restart(after) => {
                info!("Node will restart in {after:?}");

                sleep(after).await;

                info!("Spawning node");
                let new_handles = runner.spawn(node.id).await.unwrap();
                info!("Spawned");

                let new_rx_event = new_handles.tx_event.subscribe();
                let new_rx_event_bg = new_handles.tx_event.subscribe();

                bg = spawn_bg(new_rx_event_bg);
                handles = new_handles;
                rx_event = new_rx_event;
            }

            Step::OnEvent(on_event) => {
                'inner: while let Ok(event) = rx_event.recv().await {
                    match on_event(event, &mut node.state) {
                        Ok(HandlerResult::WaitForNextEvent) => {
                            continue 'inner;
                        }
                        Ok(HandlerResult::ContinueTest) => {
                            break 'inner;
                        }
                        Err(e) => {
                            bg.abort();
                            handles.engine.actor.stop(Some("Test failed".to_string()));
                            handles.app.abort();
                            handles.engine.handle.abort();

                            return TestResult::Failure(e.to_string());
                        }
                    }
                }
            }

            Step::Expect(expected) => {
                let actual = decisions.load(Ordering::SeqCst);

                bg.abort();
                handles.engine.actor.stop(Some("Test failed".to_string()));
                handles.app.abort();
                handles.engine.handle.abort();

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
                break;
            }

            Step::Fail(reason) => {
                bg.abort();
                handles.engine.actor.stop(Some("Test failed".to_string()));
                handles.app.abort();
                handles.engine.handle.abort();

                return TestResult::Failure(reason);
            }
        }
    }

    return TestResult::Success("OK".to_string());
}
