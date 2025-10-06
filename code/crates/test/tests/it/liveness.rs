// FaB: This entire test file is for Tendermint 3f+1 liveness mechanisms:
// FaB: - HIDDEN_LOCK_ROUND mechanism
// FaB: - Polka certificate rebroadcasting (2f+1 prevotes)
// FaB: - Round certificate rebroadcasting
// FaB:
// FaB: In FaB-a-la-Tendermint-bounded-square, liveness is ensured through:
// FaB: 1. Round skipping when f+1 prevotes from higher round are seen (lines 95-96)
// FaB: 2. Periodic rebroadcast of lastPrevote and proposal (lines 111-113)
// FaB:
// FaB: These tests are disabled for FaB. New FaB-specific liveness tests should be added.

/*
use std::time::Duration;

use informalsystems_malachitebft_test::TestContext;
use malachitebft_core_types::Round;
use malachitebft_test_framework::TestNode;

use crate::middlewares::PrevoteNil;
use crate::{TestBuilder, TestParams};

fn expect_round_certificate_rebroadcasts(node: &mut TestNode<TestContext>) {
    node.expect_skip_round_certificate(1, 0)
        .expect_skip_round_certificate(1, 1)
        .expect_skip_round_certificate(1, 2);
}

#[tokio::test]
async fn round_certificate_rebroadcast() {
    const FINAL_HEIGHT: u64 = 3;

    let mut test = TestBuilder::<()>::new();

    test.add_node()
        .with_middleware(PrevoteNil::when(|height, round, _| {
            height.as_u64() == 1 && round.as_i64() <= 2
        }))
        .start()
        .wait_until(1)
        .with(expect_round_certificate_rebroadcasts)
        .wait_until(FINAL_HEIGHT)
        .success();

    test.add_node()
        .start()
        .wait_until(1)
        .with(expect_round_certificate_rebroadcasts)
        .wait_until(FINAL_HEIGHT)
        .success();

    test.add_node()
        .start()
        .wait_until(1)
        .with(expect_round_certificate_rebroadcasts)
        .wait_until(FINAL_HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(30),
            TestParams {
                enable_value_sync: false,
                ..Default::default()
            },
        )
        .await
}

fn expect_hidden_lock_messages(node: &mut TestNode<TestContext>, round: Round) {
    node.expect_polka_certificate(1, round.as_u32().expect("non-nil round"));
}

#[tokio::test]
async fn polka_certificate_for_hidden_lock() {
    const FINAL_HEIGHT: u64 = 3;

    let mut test = TestBuilder::<()>::new();

    test.add_node()
        .with_middleware(PrevoteNil::when(|height, round, _| {
            height.as_u64() == 1 && round < Round::new(10) // HIDDEN_LOCK_ROUND
        }))
        .start()
        .wait_until(1)
        .with(|node| expect_hidden_lock_messages(node, Round::new(10)))
        .wait_until(FINAL_HEIGHT)
        .success();

    test.add_node()
        .start()
        .wait_until(1)
        .with(|node| expect_hidden_lock_messages(node, Round::new(10)))
        .wait_until(FINAL_HEIGHT)
        .success();

    test.add_node()
        .start()
        .wait_until(1)
        .with(|node| expect_hidden_lock_messages(node, Round::new(10)))
        .wait_until(FINAL_HEIGHT)
        .success();

    test.build()
        .run_with_params(
            Duration::from_secs(120),
            TestParams {
                enable_value_sync: false,
                ..Default::default()
            },
        )
        .await
}
*/
