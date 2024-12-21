use std::time::Duration;
use informalsystems_malachitebft_starknet_test::{init_logging, TestBuilder};

#[tokio::test]
async fn high_load_stress_test() {
    init_logging(module_path!());

    const HEIGHT: u64 = 10;
    const TRANSACTION_COUNT: u64 = 1000;

    let mut test = TestBuilder::<()>::new();

    // Start a node
    let node = test.add_node().with_voting_power(10).start();

    // Simulate high transaction load by submitting transactions directly
    for _ in 0..TRANSACTION_COUNT {
        node.submit_transaction(vec![0u8]);
    }

    // Ensure the node reaches the target height
    node.wait_until(HEIGHT).success();

    test.build().run(Duration::from_secs(60)).await;
}