use malachite_common::Round;
use malachite_vote::keeper::{Message, VoteKeeper};

use malachite_test::{Address, TestContext, ValueId, Vote};

const ADDRESS1: Address = Address::new([41; 20]);
const ADDRESS2: Address = Address::new([42; 20]);
const ADDRESS3: Address = Address::new([43; 20]);
const ADDRESS4: Address = Address::new([44; 20]);

#[test]
fn prevote_apply_nil() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(3, Default::default());
    let round = Round::new(0);

    let vote = Vote::new_prevote(round, None, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(round, None, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(round, None, ADDRESS3);
    let msg = keeper.apply_vote(vote, 1, round);
    assert_eq!(msg, Some(Message::PolkaNil));
}

#[test]
fn precommit_apply_nil() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(3, Default::default());
    let round = Round::new(0);

    let vote = Vote::new_precommit(round, None, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(round, None, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(round, None, ADDRESS3);
    let msg = keeper.apply_vote(vote, 1, round);
    assert_eq!(msg, Some(Message::PrecommitAny));
}

#[test]
fn prevote_apply_single_value() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(4, Default::default());

    let v = ValueId::new(1);
    let val = Some(v);
    let round = Round::new(0);

    let vote = Vote::new_prevote(round, val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote_nil = Vote::new_prevote(round, None, ADDRESS3);
    let msg = keeper.apply_vote(vote_nil, 1, round);
    assert_eq!(msg, Some(Message::PolkaAny));

    let vote = Vote::new_prevote(round, val, ADDRESS4);
    let msg = keeper.apply_vote(vote, 1, round);
    assert_eq!(msg, Some(Message::PolkaValue(v)));
}

#[test]
fn precommit_apply_single_value() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(4, Default::default());

    let v = ValueId::new(1);
    let val = Some(v);
    let round = Round::new(0);

    let vote = Vote::new_precommit(round, val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote_nil = Vote::new_precommit(round, None, ADDRESS3);
    let msg = keeper.apply_vote(vote_nil, 1, round);
    assert_eq!(msg, Some(Message::PrecommitAny));

    let vote = Vote::new_precommit(round, val, ADDRESS4);
    let msg = keeper.apply_vote(vote, 1, round);
    assert_eq!(msg, Some(Message::PrecommitValue(v)));
}

#[test]
fn skip_round_small_quorum_prevotes_two_vals() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(4, Default::default());

    let v = ValueId::new(1);
    let val = Some(v);
    let cur_round = Round::new(0);
    let fut_round = Round::new(1);

    let vote = Vote::new_prevote(cur_round, val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(fut_round, val, ADDRESS3);
    let msg = keeper.apply_vote(vote, 1, cur_round);
    assert_eq!(msg, Some(Message::SkipRound(Round::new(1))));
}

#[test]
fn skip_round_small_quorum_with_prevote_precommit_two_vals() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(4, Default::default());

    let v = ValueId::new(1);
    let val = Some(v);
    let cur_round = Round::new(0);
    let fut_round = Round::new(1);

    let vote = Vote::new_prevote(cur_round, val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(fut_round, val, ADDRESS3);
    let msg = keeper.apply_vote(vote, 1, cur_round);
    assert_eq!(msg, Some(Message::SkipRound(Round::new(1))));
}

#[test]
fn skip_round_full_quorum_with_prevote_precommit_two_vals() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(5, Default::default());

    let v = ValueId::new(1);
    let val = Some(v);
    let cur_round = Round::new(0);
    let fut_round = Round::new(1);

    let vote = Vote::new_prevote(cur_round, val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(fut_round, val, ADDRESS3);
    let msg = keeper.apply_vote(vote, 2, cur_round);
    assert_eq!(msg, Some(Message::SkipRound(Round::new(1))));
}

#[test]
fn no_skip_round_small_quorum_with_same_val() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(4, Default::default());

    let v = ValueId::new(1);
    let val = Some(v);
    let cur_round = Round::new(0);
    let fut_round = Round::new(1);

    let vote = Vote::new_prevote(cur_round, val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote, 1, cur_round);
    assert_eq!(msg, None);
}

#[test]
fn no_skip_round_full_quorum_with_same_val() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(5, Default::default());

    let v = ValueId::new(1);
    let val = Some(v);
    let cur_round = Round::new(0);
    let fut_round = Round::new(1);

    let vote = Vote::new_prevote(cur_round, val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote, 2, cur_round);
    assert_eq!(msg, None);
}
