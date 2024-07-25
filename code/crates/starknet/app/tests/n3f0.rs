#![allow(unused_crate_dependencies)]

use malachite_node::config::App;
use malachite_starknet_app::test::{Expected, Test, TestNode};

#[tokio::test]
pub async fn all_correct_nodes() {
    let test = Test::new(
        [
            TestNode::correct(5),
            TestNode::correct(15),
            TestNode::correct(10),
        ],
        Expected::Exactly(9),
    );

    test.run(App::Starknet).await
}
