use malachite_common::{NilOrVal, Round};
use malachite_vote::keeper::{Output, VoteKeeper};

use malachite_test::{Address, Height, TestContext, ValueId, Vote};

const ADDRESS1: Address = Address::new([41; 20]);
const ADDRESS2: Address = Address::new([42; 20]);
const ADDRESS3: Address = Address::new([43; 20]);
const ADDRESS4: Address = Address::new([44; 20]);

#[test]
fn prevote_apply_nil() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(3, Default::default());
    let height = Height::new(1);
    let round = Round::new(0);

    let vote = Vote::new_prevote(height, round, NilOrVal::Nil, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(height, round, NilOrVal::Nil, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(height, round, NilOrVal::Nil, ADDRESS3);
    let msg = keeper.apply_vote(vote, 1, round);
    assert_eq!(msg, Some(Output::PolkaNil));
}

#[test]
fn precommit_apply_nil() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(3, Default::default());
    let height = Height::new(1);
    let round = Round::new(0);

    let vote = Vote::new_precommit(height, round, NilOrVal::Nil, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(height, Round::new(0), NilOrVal::Nil, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(height, Round::new(0), NilOrVal::Nil, ADDRESS3);
    let msg = keeper.apply_vote(vote, 1, round);
    assert_eq!(msg, Some(Output::PrecommitAny));
}

#[test]
fn prevote_apply_single_value() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(4, Default::default());

    let id = ValueId::new(1);
    let val = NilOrVal::Val(id);
    let height = Height::new(1);
    let round = Round::new(0);

    let vote = Vote::new_prevote(height, Round::new(0), val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(height, Round::new(0), val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote_nil = Vote::new_prevote(height, Round::new(0), NilOrVal::Nil, ADDRESS3);
    let msg = keeper.apply_vote(vote_nil, 1, round);
    assert_eq!(msg, Some(Output::PolkaAny));

    let vote = Vote::new_prevote(height, Round::new(0), val, ADDRESS4);
    let msg = keeper.apply_vote(vote, 1, round);
    assert_eq!(msg, Some(Output::PolkaValue(id)));
}

#[test]
fn precommit_apply_single_value() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(4, Default::default());

    let id = ValueId::new(1);
    let val = NilOrVal::Val(id);
    let height = Height::new(1);
    let round = Round::new(0);

    let vote = Vote::new_precommit(height, Round::new(0), val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(height, Round::new(0), val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, round);
    assert_eq!(msg, None);

    let vote_nil = Vote::new_precommit(height, Round::new(0), NilOrVal::Nil, ADDRESS3);
    let msg = keeper.apply_vote(vote_nil, 1, round);
    assert_eq!(msg, Some(Output::PrecommitAny));

    let vote = Vote::new_precommit(height, Round::new(0), val, ADDRESS4);
    let msg = keeper.apply_vote(vote, 1, round);
    assert_eq!(msg, Some(Output::PrecommitValue(id)));
}

#[test]
fn skip_round_small_quorum_prevotes_two_vals() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(4, Default::default());

    let id = ValueId::new(1);
    let val = NilOrVal::Val(id);
    let height = Height::new(1);
    let cur_round = Round::new(0);
    let fut_round = Round::new(1);

    let vote = Vote::new_prevote(height, cur_round, val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(height, fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(height, fut_round, val, ADDRESS3);
    let msg = keeper.apply_vote(vote, 1, cur_round);
    assert_eq!(msg, Some(Output::SkipRound(Round::new(1))));
}

#[test]
fn skip_round_small_quorum_with_prevote_precommit_two_vals() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(4, Default::default());

    let id = ValueId::new(1);
    let val = NilOrVal::Val(id);
    let height = Height::new(1);
    let cur_round = Round::new(0);
    let fut_round = Round::new(1);

    let vote = Vote::new_prevote(height, cur_round, val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(height, fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(height, fut_round, val, ADDRESS3);
    let msg = keeper.apply_vote(vote, 1, cur_round);
    assert_eq!(msg, Some(Output::SkipRound(Round::new(1))));
}

#[test]
fn skip_round_full_quorum_with_prevote_precommit_two_vals() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(5, Default::default());

    let id = ValueId::new(1);
    let val = NilOrVal::Val(id);
    let height = Height::new(1);
    let cur_round = Round::new(0);
    let fut_round = Round::new(1);

    let vote = Vote::new_prevote(height, cur_round, val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(height, fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(height, fut_round, val, ADDRESS3);
    let msg = keeper.apply_vote(vote, 2, cur_round);
    assert_eq!(msg, Some(Output::SkipRound(Round::new(1))));
}

#[test]
fn no_skip_round_small_quorum_with_same_val() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(4, Default::default());

    let id = ValueId::new(1);
    let val = NilOrVal::Val(id);
    let height = Height::new(1);
    let cur_round = Round::new(0);
    let fut_round = Round::new(1);

    let vote = Vote::new_prevote(height, cur_round, val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(height, fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(height, fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote, 1, cur_round);
    assert_eq!(msg, None);
}

#[test]
fn no_skip_round_full_quorum_with_same_val() {
    let mut keeper: VoteKeeper<TestContext> = VoteKeeper::new(5, Default::default());

    let id = ValueId::new(1);
    let val = NilOrVal::Val(id);
    let height = Height::new(1);
    let cur_round = Round::new(0);
    let fut_round = Round::new(1);

    let vote = Vote::new_prevote(height, cur_round, val, ADDRESS1);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_prevote(height, fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote.clone(), 1, cur_round);
    assert_eq!(msg, None);

    let vote = Vote::new_precommit(height, fut_round, val, ADDRESS2);
    let msg = keeper.apply_vote(vote, 2, cur_round);
    assert_eq!(msg, None);
}
