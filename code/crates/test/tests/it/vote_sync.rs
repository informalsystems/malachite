use std::time::Duration;

use malachitebft_config::{ValuePayload, VoteSyncMode};

use crate::{TestBuilder, TestParams};

// NOTE: These tests are very similar to the Sync tests, with the difference that
//       all nodes have the same voting power and therefore get stuck when one of them dies.

pub async fn crash_restart_from_start(params: TestParams) {
    const HEIGHT: u64 = 10;
    const CRASH_HEIGHT: u64 = 4;

    let mut test = TestBuilder::<()>::new();

    test.add_node().start().wait_until(HEIGHT).success();
    test.add_node().start().wait_until(HEIGHT).success();

    test.add_node()
        .start()
        // Wait until the node reaches height 4...
        .wait_until(CRASH_HEIGHT)
        // ...then kill it
        .crash()
        // Reset the database so that the node has to do Sync from height 1
        .reset_db()
        // After that, it waits 5 seconds before restarting the node
        .restart_after(Duration::from_secs(5))
        // Expect a vote set request for height 4
        .expect_vote_set_request(CRASH_HEIGHT)
        // Wait until the node reached the expected height
        .wait_until(HEIGHT)
        // Record a successful test for this node
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(60), // Timeout for the whole test
            TestParams {
                vote_sync_mode: Some(VoteSyncMode::RequestResponse),
                timeout_step: Duration::from_secs(5),
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
#[ignore] // Test app does not support proposal-only mode
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
    const CRASH_HEIGHT: u64 = 4;

    let mut test = TestBuilder::<()>::new();

    test.add_node()
        .start()
        .wait_until(CRASH_HEIGHT)
        .wait_until(HEIGHT)
        .success();

    test.add_node()
        .start()
        .wait_until(CRASH_HEIGHT)
        .wait_until(HEIGHT)
        .success();

    test.add_node()
        .start()
        .wait_until(CRASH_HEIGHT)
        .crash()
        // We do not reset the database so that the node can restart from the latest height
        .restart_after(Duration::from_secs(5))
        .expect_vote_set_request(CRASH_HEIGHT)
        .wait_until(HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(60),
            TestParams {
                vote_sync_mode: Some(VoteSyncMode::RequestResponse),
                timeout_step: Duration::from_secs(5),
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
pub async fn start_late() {
    const HEIGHT: u64 = 5;

    let mut test = TestBuilder::<()>::new();

    test.add_node().start().wait_until(HEIGHT).success();
    test.add_node().start().wait_until(HEIGHT).success();

    test.add_node()
        .start_after(1, Duration::from_secs(10))
        .expect_vote_set_request(1)
        .wait_until(HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(60),
            TestParams {
                enable_value_sync: true, // Enable ValueSync to allow node to catch up to latest height
                vote_sync_mode: Some(VoteSyncMode::RequestResponse),
                timeout_step: Duration::from_secs(5),
                ..Default::default()
            },
        )
        .await
}
