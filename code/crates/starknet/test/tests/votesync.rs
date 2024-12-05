use std::time::Duration;

use malachite_config::ValuePayload;
use malachite_starknet_test::{Test, TestNode, TestParams};

// NOTE: These tests are very similar to the BlockSync tests, with the difference that
//       all nodes have the same voting power and therefore get stuck when one of them dies.

pub async fn crash_restart_from_start(params: TestParams) {
    const HEIGHT: u64 = 10;

    let n1 = TestNode::new(1).start().wait_until(HEIGHT).success();
    let n2 = TestNode::new(2).start().wait_until(HEIGHT).success();

    let n3 = TestNode::new(3)
        .start()
        // Wait until the node reaches height 4...
        .wait_until(4)
        // ...then kill it
        .crash()
        // Reset the database so that the node has to do BlockSync from height 1
        .reset_db()
        // After that, it waits 5 seconds before restarting the node
        .restart_after(Duration::from_secs(5))
        // Wait until the node reached the expected height
        .wait_until(HEIGHT)
        // Record a successful test for this node
        .success();

    Test::new([n1, n2, n3])
        .run_with_custom_config(
            Duration::from_secs(60), // Timeout for the whole test
            TestParams {
                enable_blocksync: true, // Enable BlockSync
                ..params
            },
        )
        .await
}

#[tokio::test]
#[ignore] // Test is failing
pub async fn crash_restart_from_start_parts_only() {
    let params = TestParams {
        value_payload: ValuePayload::PartsOnly,
        ..Default::default()
    };

    crash_restart_from_start(params).await
}

#[tokio::test]
#[ignore] // Test is failing
pub async fn crash_restart_from_start_proposal_only() {
    let params = TestParams {
        value_payload: ValuePayload::ProposalOnly,
        ..Default::default()
    };

    crash_restart_from_start(params).await
}

#[tokio::test]
#[ignore] // Test is failing
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

    let n1 = TestNode::new(1).start().wait_until(HEIGHT).success();
    let n2 = TestNode::new(2).start().wait_until(HEIGHT).success();
    let n3 = TestNode::new(3)
        .start()
        .wait_until(2)
        .crash()
        // We do not reset the database so that the node can restart from the latest height
        .restart_after(Duration::from_secs(5))
        .wait_until(HEIGHT)
        .success();

    Test::new([n1, n2, n3])
        .run_with_custom_config(
            Duration::from_secs(60),
            TestParams {
                enable_blocksync: true,
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
#[ignore] // Test is failing
pub async fn start_late() {
    const HEIGHT: u64 = 5;

    let n1 = TestNode::new(1).start().wait_until(HEIGHT * 2).success();
    let n2 = TestNode::new(2).start().wait_until(HEIGHT * 2).success();
    let n3 = TestNode::new(3)
        .start_after(1, Duration::from_secs(10))
        .wait_until(HEIGHT)
        .success();

    Test::new([n1, n2, n3])
        .run_with_custom_config(
            Duration::from_secs(30),
            TestParams {
                enable_blocksync: true,
                ..Default::default()
            },
        )
        .await
}
