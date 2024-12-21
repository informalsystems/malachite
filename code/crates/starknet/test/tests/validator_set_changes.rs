use std::time::Duration;
use informalsystems_malachitebft_starknet_test::{init_logging, TestBuilder};

#[tokio::test]
async fn dynamic_validator_set_changes() {
    init_logging(module_path!());

    const HEIGHT: u64 = 10;

    let mut test = TestBuilder::<()>::new();

    // Configure all nodes at once
    test.add_node().with_voting_power(10).start().wait_until(5).wait_until(HEIGHT).success();
    test.add_node().with_voting_power(10).start().wait_until(5).wait_until(HEIGHT).success();
    test.add_node().with_voting_power(10).start().wait_until(HEIGHT).success();

    test.build().run(Duration::from_secs(60)).await;
}