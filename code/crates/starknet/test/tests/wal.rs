use std::time::Duration;

use eyre::bail;
use malachite_config::ValuePayload;
use tracing::info;

use malachite_actors::util::events::Event;
use malachite_common::SignedVote;
use malachite_consensus::ValueToPropose;
use malachite_starknet_host::types::MockContext;
use malachite_starknet_test::{init_logging, HandlerResult, Test, TestNode, TestParams};

#[tokio::test]
async fn proposer_crashes_after_proposing_parts_only() {
    proposer_crashes_after_proposing(TestParams {
        value_payload: ValuePayload::PartsOnly,
        ..TestParams::default()
    })
    .await
}

#[tokio::test]
async fn proposer_crashes_after_proposing_proposal_and_parts() {
    proposer_crashes_after_proposing(TestParams {
        value_payload: ValuePayload::ProposalAndParts,
        ..TestParams::default()
    })
    .await
}

#[tokio::test]
async fn proposer_crashes_after_proposing_proposal_only() {
    proposer_crashes_after_proposing(TestParams {
        value_payload: ValuePayload::ProposalOnly,
        ..TestParams::default()
    })
    .await
}

async fn proposer_crashes_after_proposing(params: TestParams) {
    init_logging(module_path!());

    #[derive(Clone, Debug, Default)]
    struct State {
        first_proposed_value: Option<ValueToPropose<MockContext>>,
    }

    const CRASH_HEIGHT: u64 = 4;

    let n1 = TestNode::with_state(1, State::default())
        .vp(10)
        .start()
        .success();

    let n2 = TestNode::with_state(3, State::default())
        .vp(10)
        .start()
        .success();

    let n3 = TestNode::with_state(3, State::default())
        .vp(40)
        .start()
        .wait_until(CRASH_HEIGHT)
        // Wait until this node proposes a value
        .on_event(|event, state| match event {
            Event::ProposedValue(value) => {
                info!("Proposer proposed block: {:?}", value.value);
                state.first_proposed_value = Some(value);
                Ok(HandlerResult::ContinueTest)
            }
            _ => Ok(HandlerResult::WaitForNextEvent),
        })
        // Crash right after
        .crash()
        // Restart after 5 seconds
        .restart_after(Duration::from_secs(5))
        // Check that we replay messages from the WAL
        .expect_wal_replay(CRASH_HEIGHT)
        // Wait until it proposes a value again, while replaying WAL
        // Check that it is the same value as the first time
        .on_proposed_value(|value, state| {
            let Some(first_value) = state.first_proposed_value.as_ref() else {
                bail!("Proposer did not propose a block");
            };

            if first_value.value == value.value {
                info!("Proposer re-proposed the same block: {:?}", value.value);
                Ok(HandlerResult::ContinueTest)
            } else {
                bail!(
                    "Proposer just equivocated: expected {:?}, got {:?}",
                    first_value,
                    value.value
                )
            }
        })
        .success();

    Test::new([n1, n2, n3])
        .run_with_custom_config(
            Duration::from_secs(30),
            TestParams {
                enable_blocksync: false,
                ..params
            },
        )
        .await
}

#[tokio::test]
async fn non_proposer_crashes_after_voting_parts_only() {
    non_proposer_crashes_after_voting(TestParams {
        value_payload: ValuePayload::PartsOnly,
        ..TestParams::default()
    })
    .await
}

#[tokio::test]
async fn non_proposer_crashes_after_voting_proposal_and_parts() {
    non_proposer_crashes_after_voting(TestParams {
        value_payload: ValuePayload::ProposalAndParts,
        ..TestParams::default()
    })
    .await
}

#[tokio::test]
async fn non_proposer_crashes_after_voting_proposal_only() {
    non_proposer_crashes_after_voting(TestParams {
        value_payload: ValuePayload::ProposalOnly,
        ..TestParams::default()
    })
    .await
}

async fn non_proposer_crashes_after_voting(params: TestParams) {
    init_logging(module_path!());

    #[derive(Clone, Debug, Default)]
    struct State {
        first_vote: Option<SignedVote<MockContext>>,
    }

    const CRASH_HEIGHT: u64 = 3;

    let n1 = TestNode::with_state(1, State::default())
        .vp(10)
        .start()
        .success();

    let n2 = TestNode::with_state(2, State::default())
        .vp(10)
        .start()
        .success();

    let n3 = TestNode::with_state(3, State::default())
        .vp(40)
        .start()
        .wait_until(CRASH_HEIGHT)
        // Wait until this node proposes a value
        .on_vote(|vote, state| {
            info!("Non-proposer voted");
            state.first_vote = Some(vote);

            Ok(HandlerResult::ContinueTest)
        })
        // Crash right after
        .crash()
        // Restart after 5 seconds
        .restart_after(Duration::from_secs(5))
        // Check that we replay messages from the WAL
        .expect_wal_replay(CRASH_HEIGHT)
        // Wait until it proposes a value again, while replaying WAL
        // Check that it is the same value as the first time
        .on_vote(|vote, state| {
            let Some(first_vote) = state.first_vote.as_ref() else {
                bail!("Non-proposer did not vote")
            };

            if first_vote.block_hash == vote.block_hash {
                info!("Non-proposer voted the same way: {first_vote:?}");
                Ok(HandlerResult::ContinueTest)
            } else {
                bail!(
                    "Non-proposer just equivocated: expected {:?}, got {:?}",
                    first_vote,
                    vote.block_hash
                )
            }
        })
        .success();

    Test::new([n1, n2, n3])
        .run_with_custom_config(
            Duration::from_secs(30),
            TestParams {
                enable_blocksync: false,
                ..params
            },
        )
        .await
}
