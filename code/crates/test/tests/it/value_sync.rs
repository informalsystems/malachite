use crate::{TestBuilder, TestParams};
use bytes::Bytes;
use bytesize::ByteSize;
use eyre::bail;
use informalsystems_malachitebft_test::decided_value::DecidedValue;
use informalsystems_malachitebft_test::middleware::{Middleware, RotateEpochValidators};
use informalsystems_malachitebft_test::TestContext;
use informalsystems_malachitebft_test::{Height, Value, ValueId};
use malachitebft_config::ValuePayload;
use malachitebft_core_consensus::ProposedValue;
use malachitebft_core_types::{CommitCertificate, Round};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub async fn crash_restart_from_start(params: TestParams) {
    const HEIGHT: u64 = 6;
    const CRASH_HEIGHT: u64 = 4;

    let mut test = TestBuilder::<()>::new();

    // Node 1 starts with 10 voting power.
    test.add_node()
        .with_voting_power(10)
        .start()
        // Wait until it reaches height 10
        .wait_until(HEIGHT)
        // Record a successful test for this node
        .success();

    // Node 2 starts with 10 voting power, in parallel with node 1 and with the same behaviour
    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .success();

    // Node 3 starts with 5 voting power, in parallel with node 1 and 2.
    test.add_node()
        .with_voting_power(5)
        .start()
        // Wait until the node reaches height 2...
        .wait_until(CRASH_HEIGHT)
        // ...and then kills it
        .crash()
        // Reset the database so that the node has to do Sync from height 1
        .reset_db()
        // After that, it waits 5 seconds before restarting the node
        .restart_after(Duration::from_secs(5))
        // Wait until the node reached the expected height
        .wait_until(HEIGHT)
        // Record a successful test for this node
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(60), // Timeout for the whole test
            TestParams {
                enable_value_sync: true, // Enable Sync
                ..params
            },
        )
        .await
}

#[tokio::test]
pub async fn crash_restart_from_start_parts_only() {
    let params = TestParams {
        value_payload: ValuePayload::PartsOnly,
        ..Default::default()
    };

    crash_restart_from_start(params).await
}

#[tokio::test]
#[ignore]
pub async fn crash_restart_from_start_proposal_only() {
    let params = TestParams {
        value_payload: ValuePayload::ProposalOnly,
        ..Default::default()
    };

    crash_restart_from_start(params).await
}

#[tokio::test]
pub async fn crash_restart_from_start_proposal_and_parts() {
    let params = TestParams {
        value_payload: ValuePayload::ProposalAndParts,
        ..Default::default()
    };

    crash_restart_from_start(params).await
}

#[tokio::test]
pub async fn crash_restart_from_latest() {
    const HEIGHT: u64 = 10;

    let mut test = TestBuilder::<()>::new();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .success();
    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .success();

    test.add_node()
        .with_voting_power(5)
        .start()
        .wait_until(2)
        .crash()
        // We do not reset the database so that the node can restart from the latest height
        .restart_after(Duration::from_secs(5))
        .wait_until(HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(60),
            TestParams {
                enable_value_sync: true,
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
pub async fn aggressive_pruning() {
    const HEIGHT: u64 = 15;

    let mut test = TestBuilder::<()>::new();

    // Node 1 starts with 10 voting power.
    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .success();
    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .success();

    test.add_node()
        .with_voting_power(5)
        .start()
        .wait_until(2)
        .crash()
        .reset_db()
        .restart_after(Duration::from_secs(5))
        .wait_until(HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(60), // Timeout for the whole test
            TestParams {
                enable_value_sync: true, // Enable Sync
                max_retain_blocks: 10,   // Prune blocks older than 10
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
pub async fn start_late() {
    const HEIGHT: u64 = 5;

    let mut test = TestBuilder::<()>::new();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT * 2)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT * 2)
        .success();

    test.add_node()
        .with_voting_power(5)
        .start_after(1, Duration::from_secs(10))
        .wait_until(HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(30),
            TestParams {
                enable_value_sync: true,
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
pub async fn start_late_parallel_requests() {
    const HEIGHT: u64 = 10;

    let mut test = TestBuilder::<()>::new();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT * 2)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT * 2)
        .success();

    test.add_node()
        .with_voting_power(5)
        .start_after(1, Duration::from_secs(10))
        .wait_until(HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(30),
            TestParams {
                enable_value_sync: true,
                parallel_requests: 5,
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
pub async fn start_late_parallel_requests_with_batching() {
    const HEIGHT: u64 = 10;

    let mut test = TestBuilder::<()>::new();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT * 2)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT * 2)
        .success();

    test.add_node()
        .with_voting_power(0)
        .start_after(1, Duration::from_secs(10))
        .wait_until(HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(30),
            TestParams {
                enable_value_sync: true,
                parallel_requests: 2,
                batch_size: 2,
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
pub async fn start_late_rotate_epoch_validator_set() {
    const HEIGHT: u64 = 20;

    let mut test = TestBuilder::<()>::new();

    test.add_node()
        .with_voting_power(10)
        .with_middleware(RotateEpochValidators {
            selection_size: 2,
            epochs_limit: 5,
        })
        .start()
        .wait_until(HEIGHT)
        .success();

    test.add_node()
        .with_voting_power(10)
        .with_middleware(RotateEpochValidators {
            selection_size: 2,
            epochs_limit: 5,
        })
        .start()
        .wait_until(HEIGHT)
        .success();

    test.add_node()
        .with_voting_power(10)
        .with_middleware(RotateEpochValidators {
            selection_size: 2,
            epochs_limit: 5,
        })
        .start()
        .wait_until(HEIGHT)
        .success();

    // Add 2 full nodes with one starting late
    test.add_node()
        .full_node()
        .with_middleware(RotateEpochValidators {
            selection_size: 2,
            epochs_limit: 5,
        })
        .start()
        .wait_until(HEIGHT)
        .success();
    test.add_node()
        .full_node()
        .with_middleware(RotateEpochValidators {
            selection_size: 2,
            epochs_limit: 5,
        })
        .start_after(1, Duration::from_secs(5))
        .wait_until(HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(30),
            TestParams {
                enable_value_sync: true,
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
pub async fn sync_only_fullnode_without_consensus() {
    const HEIGHT: u64 = 8;

    let mut test = TestBuilder::<()>::new();

    // First two nodes are normal validators that will drive consensus
    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .success();

    // Third node is a sync-only full node (0 voting power, consensus disabled)
    // It should be able to sync values but not participate in consensus
    test.add_node()
        .full_node()
        .disable_consensus()
        .start_after(1, Duration::from_secs(5)) // Start late to force syncing
        .wait_until(HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(45),
            // NOTE: consensus is enabled by default for other nodes
            TestParams {
                enable_value_sync: true,
                parallel_requests: 3,
                ..Default::default()
            },
        )
        .await
}

#[derive(Debug)]
struct ResetHeight {
    reset_height: u64,
    reset: AtomicBool,
}

impl ResetHeight {
    fn new(reset_height: u64) -> Self {
        Self {
            reset_height,
            reset: AtomicBool::new(false),
        }
    }
}

impl Middleware for ResetHeight {
    fn on_commit(
        &self,
        _ctx: &TestContext,
        certificate: &CommitCertificate<TestContext>,
        proposal: &ProposedValue<TestContext>,
    ) -> Result<(), eyre::Report> {
        assert_eq!(certificate.height, proposal.height);

        if certificate.height.as_u64() == self.reset_height
            && !self.reset.swap(true, Ordering::SeqCst)
        {
            bail!("Simulating commit failure");
        }

        Ok(())
    }
}

#[tokio::test]
pub async fn reset_height() {
    const HEIGHT: u64 = 10;
    const RESET_HEIGHT: u64 = 1;
    let mut test = TestBuilder::<()>::new();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT * 2)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT * 2)
        .success();

    test.add_node()
        .with_voting_power(0)
        .with_middleware(ResetHeight::new(RESET_HEIGHT))
        .start_after(1, Duration::from_secs(10))
        .wait_until(RESET_HEIGHT) // First time reaching height
        .wait_until(RESET_HEIGHT)
        .wait_until(HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(30),
            TestParams {
                enable_value_sync: true,
                parallel_requests: 3,
                batch_size: 2,
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
pub async fn response_size_limit_exceeded() {
    const HEIGHT: u64 = 5;

    let mut test = TestBuilder::<()>::new();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .success();

    // Node 3 starts with 5 voting power, in parallel with node 1 and 2.
    test.add_node()
        .with_voting_power(5)
        .start()
        // Wait until the node reaches height 2...
        .wait_until(2)
        // ...and then kills it
        .crash()
        // Reset the database so that the node has to do Sync from height 1
        .reset_db()
        // After that, it waits 5 seconds before restarting the node
        .restart_after(Duration::from_secs(5))
        // Wait until the node reached the expected height
        .wait_until(HEIGHT)
        // Record a successful test for this node
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(60),
            TestParams {
                enable_value_sync: true,
                // Values are around ~900 bytes, so this `max_response_size` in combination
                // with a `batch_size` of 2 leads to having a syncing peer sending partial responses.
                max_response_size: ByteSize::b(1000),
                // Values are around ~900 bytes, so we canNOT have more than one value in a response.
                // In other words, if `max_response_size` is not respected, node 3 would not have been
                // able to sync in this test.
                rpc_max_size: ByteSize::b(1000),
                batch_size: 2,
                parallel_requests: 1,
                ..Default::default()
            },
        )
        .await
}

#[derive(Debug)]
struct InvalidDecidedValue {
    heights: Vec<Height>,
}

impl InvalidDecidedValue {
    // returns invalid values for the provided heights
    fn new(heights: Vec<Height>) -> Self {
        Self { heights }
    }
}

impl Middleware for InvalidDecidedValue {
    fn get_decided_value(&self, height: Height) -> Option<DecidedValue> {
        if self.heights.contains(&height) {
            Some(DecidedValue {
                value: Value {
                    value: 1,
                    extensions: Bytes::new(),
                },
                certificate: CommitCertificate::new(
                    Height::new(1),
                    Round::new(1),
                    ValueId::new(1),
                    vec![],
                ),
            })
        } else {
            None
        }
    }
}

#[tokio::test]
pub async fn invalid_values() {
    const HEIGHT: u64 = 10;
    let mut test = TestBuilder::<()>::new();

    // simple helper function
    fn numbers_to_heights(numbers: Vec<u64>) -> Vec<Height> {
        numbers.into_iter().map(Height::new).collect()
    }

    // first node returns invalid values for heights 1, 3, ...
    test.add_node()
        .with_voting_power(10)
        .with_middleware(InvalidDecidedValue::new(numbers_to_heights(vec![
            1, 3, 5, 7, 9,
        ])))
        .start()
        .wait_until(HEIGHT)
        .success();

    // second node returns invalid values for heights 2, 4, ...
    test.add_node()
        .with_voting_power(10)
        .with_middleware(InvalidDecidedValue::new(numbers_to_heights(vec![
            2, 4, 6, 8, 10,
        ])))
        .start()
        .wait_until(HEIGHT * 2)
        .success();

    // third node will restart after reaching height 10, and hence would need to sync the first
    // 10 values from the first 2 nodes
    test.add_node()
        .with_voting_power(0)
        .start()
        // Wait until the node reaches height 10
        .wait_until(HEIGHT)
        // ...and then kill it
        .crash()
        // Reset the database so that the node has to do Sync from height 1
        .reset_db()
        // After that, it waits 5 seconds before restarting the node
        .restart_after(Duration::from_secs(5))
        // Wait until the node reached the expected height
        .wait_until(HEIGHT * 2)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(30),
            TestParams {
                enable_value_sync: true,
                parallel_requests: 3,
                enable_discovery: true,
                exclude_from_persistent_peers: vec![4], // Node 4 is a new validator, others don't have it as persistent peer
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
pub async fn validator_persistent_peer_reconnection_discovery_disabled() {
    const HEIGHT: u64 = 10;

    let mut test = TestBuilder::<()>::new();

    // Node 1-3: validators that will restart
    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .crash()
        .restart_after(Duration::from_secs(3))
        .wait_until(HEIGHT + 5)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .crash()
        .restart_after(Duration::from_secs(3))
        .wait_until(HEIGHT + 5)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .crash()
        .restart_after(Duration::from_secs(3))
        .wait_until(HEIGHT + 5)
        .success();

    // Node 4: validator that that syncs and needs to reconnect after all validators have restarted
    test.add_node()
        .with_voting_power(5)
        .start_after(1, Duration::from_secs(12))
        .wait_until(HEIGHT + 5)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(30),
            TestParams {
                enable_value_sync: true,
                parallel_requests: 3,
                batch_size: 3,
                enable_discovery: false,
                exclude_from_persistent_peers: vec![4], // Node 4 is a new validator, others don't have it as persistent peer
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
pub async fn full_node_persistent_peer_reconnection_discovery_enabled() {
    const HEIGHT: u64 = 10;

    let mut test = TestBuilder::<()>::new();

    // Node 1-3: validators that will restart
    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .crash()
        .restart_after(Duration::from_secs(3))
        .wait_until(HEIGHT + 5)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .crash()
        .restart_after(Duration::from_secs(3))
        .wait_until(HEIGHT + 5)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .crash()
        .restart_after(Duration::from_secs(3))
        .wait_until(HEIGHT + 5)
        .success();

    // Node 4: full node that that syncs and needs to reconnect after all validators have restarted
    test.add_node()
        .full_node()
        .start_after(1, Duration::from_secs(3))
        .wait_until(HEIGHT + 5)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(30),
            TestParams {
                enable_value_sync: true,
                parallel_requests: 3,
                enable_discovery: true,
                // Node 4 is a full node, other validators don't have it as persistent peer
                exclude_from_persistent_peers: vec![4],
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
pub async fn full_node_persistent_peer_reconnection_discovery_disabled() {
    const HEIGHT: u64 = 10;

    let mut test = TestBuilder::<()>::new();

    // Node 1-3: validators that will restart
    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .crash()
        .restart_after(Duration::from_secs(3))
        .wait_until(HEIGHT + 5)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .crash()
        .restart_after(Duration::from_secs(3))
        .wait_until(HEIGHT + 5)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .crash()
        .restart_after(Duration::from_secs(3))
        .wait_until(HEIGHT + 5)
        .success();

    // Node 4: full node that syncs and needs to reconnect after all validators have restarted
    test.add_node()
        .full_node()
        .start_after(1, Duration::from_secs(3))
        .wait_until(HEIGHT + 5)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(30),
            TestParams {
                enable_value_sync: true,
                parallel_requests: 3,
                enable_discovery: false,
                // Node 4 is a full node, other validators don't have it as persistent peer
                exclude_from_persistent_peers: vec![4],
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
pub async fn full_node_sync_after_all_persistent_peer_restart() {
    const HEIGHT: u64 = 10;

    let mut test = TestBuilder::<()>::new();

    // Node 1-3: validators that will restart
    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .crash()
        .restart_after(Duration::from_secs(4))
        .wait_until(HEIGHT + 5)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .crash()
        .restart_after(Duration::from_secs(4))
        .wait_until(HEIGHT + 5)
        .success();

    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .crash()
        .restart_after(Duration::from_secs(4))
        .wait_until(HEIGHT + 5)
        .success();

    // Node 4: full node that syncs and should resume syncing all validators have restarted
    test.add_node()
        .full_node()
        .start_after(1, Duration::from_secs(3))
        .wait_until(HEIGHT + 5)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(30),
            TestParams {
                enable_value_sync: true,
                parallel_requests: 3,
                ..Default::default()
            },
        )
        .await
}
