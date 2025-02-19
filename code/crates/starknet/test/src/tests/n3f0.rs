use std::time::Duration;

use malachitebft_test_framework::TestParams;

use crate::TestBuilder;

#[tokio::test]
pub async fn all_correct_nodes() {
    const HEIGHT: u64 = 5;

    let mut test = TestBuilder::<()>::new();

    test.add_node().start().wait_until(HEIGHT).success();
    test.add_node().start().wait_until(HEIGHT).success();
    test.add_node().start().wait_until(HEIGHT).success();

    test.build()
        .run_with_params(
            Duration::from_secs(30), // Timeout for the whole test
            TestParams {
                enable_sync: false, // Enable Sync
                ..Default::default()
            },
        )
        .await
}
