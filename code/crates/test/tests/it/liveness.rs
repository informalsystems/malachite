use std::time::Duration;

use informalsystems_malachitebft_test::TestContext;
use malachitebft_config::VoteSyncMode;
use malachitebft_core_types::VoteType;
use malachitebft_test_framework::TestNode;

use crate::middlewares::PrevoteNil;
use crate::{TestBuilder, TestParams};

fn expect_rebroadcasts(node: &mut TestNode<TestContext>) {
    use VoteType::*;

    node.expect_vote_rebroadcast(1, 0, Prevote)
        // Prevoted in round 0
        .expect_vote_rebroadcast(1, 0, Prevote)
        .expect_vote_rebroadcast(1, 0, Precommit)
        .expect_round_certificate(1, 0)
        // Precommited in round 0, has round certificate
        .expect_vote_rebroadcast(1, 0, Prevote)
        .expect_vote_rebroadcast(1, 0, Precommit)
        .expect_round_certificate(1, 0)
        // Round 0 is rebroadcast again
        .expect_vote_rebroadcast(1, 1, Prevote)
        .expect_vote_rebroadcast(1, 0, Precommit)
        .expect_round_certificate(1, 0)
        // Prevoted in round 1
        .expect_vote_rebroadcast(1, 1, Prevote)
        .expect_vote_rebroadcast(1, 1, Precommit)
        .expect_round_certificate(1, 1)
        // Precommited in round 1, has round certificate
        .expect_vote_rebroadcast(1, 1, Prevote)
        .expect_vote_rebroadcast(1, 1, Precommit)
        .expect_round_certificate(1, 1)
        // Round 1 is rebroadcast again
        .expect_vote_rebroadcast(1, 2, Prevote)
        .expect_vote_rebroadcast(1, 1, Precommit)
        .expect_round_certificate(1, 1)
        // Prevoted in round 2
        .expect_vote_rebroadcast(1, 2, Prevote)
        .expect_vote_rebroadcast(1, 2, Precommit)
        .expect_round_certificate(1, 2)
        // Precommited in round 2, has round certificate
        .expect_vote_rebroadcast(1, 2, Prevote)
        .expect_vote_rebroadcast(1, 2, Precommit)
        .expect_round_certificate(1, 2);
    // Round 2 is rebroadcast again
}

#[tokio::test]
async fn multi_rounds() {
    const FINAL_HEIGHT: u64 = 3;

    let mut test = TestBuilder::<()>::new();

    test.add_node()
        .with_middleware(PrevoteNil::when(|height, round, _| {
            height.as_u64() == 1 && round.as_i64() <= 2
        }))
        .start()
        .wait_until(1)
        .with(expect_rebroadcasts)
        .wait_until(FINAL_HEIGHT)
        .success();

    test.add_node()
        .start()
        .wait_until(1)
        .with(expect_rebroadcasts)
        .wait_until(FINAL_HEIGHT)
        .success();

    test.add_node()
        .start()
        .wait_until(1)
        .with(expect_rebroadcasts)
        .wait_until(FINAL_HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(30),
            TestParams {
                enable_value_sync: false,
                vote_sync_mode: Some(VoteSyncMode::Rebroadcast),
                ..Default::default()
            },
        )
        .await
}
