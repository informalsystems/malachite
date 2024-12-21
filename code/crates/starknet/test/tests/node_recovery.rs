use std::time::Duration;
use informalsystems_malachitebft_starknet_test::{init_logging, TestBuilder};

#[tokio::test]
async fn node_recovery_after_crash() {
    init_logging(module_path!());

    const HEIGHT: u64 = 10;

    let mut test = TestBuilder::<()>::new();

    // Node 1 starts and reaches a certain height
    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(HEIGHT)
        .success();

    // Node 2 starts, crashes, and then recovers
    test.add_node()
        .with_voting_power(10)
        .start()
        .wait_until(5)
        .crash()
        .restart_after(Duration::from_secs(5))
        .wait_until(HEIGHT)
        .success();

    test.build().run(Duration::from_secs(60)).await;
}