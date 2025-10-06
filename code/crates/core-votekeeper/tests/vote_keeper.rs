// FaB: Tests for FaB-a-la-Tendermint-bounded-square vote keeper (n=5f+1)
// All precommit-related tests removed - FaB only uses prevotes

use malachitebft_core_types::{NilOrVal, Round, SignedVote};

use informalsystems_malachitebft_core_votekeeper::keeper::{Output, VoteKeeper};

use malachitebft_test::{
    Address, Height, PrivateKey, Signature, TestContext, Validator, ValidatorSet, ValueId, Vote,
};

fn setup<const N: usize>(vp: [u64; N]) -> ([Address; N], VoteKeeper<TestContext>) {
    let mut addrs = [Address::new([0; 20]); N];
    let mut vals = Vec::with_capacity(N);
    for i in 0..N {
        let pk = PrivateKey::from([i as u8; 32]);
        addrs[i] = Address::from_public_key(&pk.public_key());
        vals.push(Validator::new(pk.public_key(), vp[i]));
    }
    let keeper = VoteKeeper::new(ValidatorSet::new(vals), Default::default());
    (addrs, keeper)
}

fn new_signed_prevote(
    height: Height,
    round: Round,
    value: NilOrVal<ValueId>,
    addr: Address,
) -> SignedVote<TestContext> {
    SignedVote::new(
        Vote::new_prevote(height, round, value, addr),
        Signature::test(),
    )
}

#[test]
fn fab_certificate_for_nil() {
    // FaB: For n=5f+1, need 5 out of 5 for 4f+1 certificate
    let ([addr1, addr2, addr3, addr4, addr5], mut keeper) = setup([1, 1, 1, 1, 1]);

    let height = Height::new(1);
    let round = Round::new(0);

    let vote = new_signed_prevote(height, round, NilOrVal::Nil, addr1);
    assert_eq!(keeper.apply_vote(vote, round), None);

    let vote = new_signed_prevote(height, round, NilOrVal::Nil, addr2);
    assert_eq!(keeper.apply_vote(vote, round), None);

    let vote = new_signed_prevote(height, round, NilOrVal::Nil, addr3);
    assert_eq!(keeper.apply_vote(vote, round), None);

    let vote = new_signed_prevote(height, round, NilOrVal::Nil, addr4);
    assert_eq!(keeper.apply_vote(vote, round), None);

    // FaB: 5th vote reaches 4f+1 certificate (CertificateAny)
    let vote = new_signed_prevote(height, round, NilOrVal::Nil, addr5);
    assert_eq!(keeper.apply_vote(vote, round), Some(Output::CertificateAny));
}

#[test]
fn fab_certificate_for_value() {
    // FaB: Test reaching 4f+1 certificate for same value
    let ([addr1, addr2, addr3, addr4, addr5], mut keeper) = setup([1, 1, 1, 1, 1]);

    let id = ValueId::new(1);
    let val = NilOrVal::Val(id);
    let height = Height::new(1);
    let round = Round::new(0);

    let vote = new_signed_prevote(height, round, val, addr1);
    assert_eq!(keeper.apply_vote(vote, round), None);

    let vote = new_signed_prevote(height, round, val, addr2);
    assert_eq!(keeper.apply_vote(vote, round), None);

    let vote = new_signed_prevote(height, round, val, addr3);
    assert_eq!(keeper.apply_vote(vote, round), None);

    let vote = new_signed_prevote(height, round, val, addr4);
    assert_eq!(keeper.apply_vote(vote, round), None);

    // FaB: 5th vote reaches 4f+1 certificate (CertificateValue)
    let vote = new_signed_prevote(height, round, val, addr5);
    assert_eq!(keeper.apply_vote(vote, round), Some(Output::CertificateValue(id)));
}

#[test]
fn fab_certificate_without_quorum() {
    // FaB: Test 4f+1 certificate with votes distributed (no 2f+1 lock)
    let ([addr1, addr2, addr3, addr4, addr5], mut keeper) = setup([1, 1, 1, 1, 1]);

    let id1 = ValueId::new(1);
    let id2 = ValueId::new(2);
    let val1 = NilOrVal::Val(id1);
    let val2 = NilOrVal::Val(id2);
    let height = Height::new(1);
    let round = Round::new(0);

    // 2 votes for value1
    let vote = new_signed_prevote(height, round, val1, addr1);
    assert_eq!(keeper.apply_vote(vote, round), None);
    let vote = new_signed_prevote(height, round, val1, addr2);
    assert_eq!(keeper.apply_vote(vote, round), None);

    // 2 votes for value2
    let vote = new_signed_prevote(height, round, val2, addr3);
    assert_eq!(keeper.apply_vote(vote, round), None);
    let vote = new_signed_prevote(height, round, val2, addr4);
    assert_eq!(keeper.apply_vote(vote, round), None);

    // FaB: 5th vote for nil reaches 4f+1 total, but no 2f+1 lock for any specific value
    // Should emit CertificateAny (driver will detect no lock using find_lock_in_certificate)
    let vote = new_signed_prevote(height, round, NilOrVal::Nil, addr5);
    assert_eq!(keeper.apply_vote(vote, round), Some(Output::CertificateAny));
}

#[test]
fn fab_skip_round() {
    // FaB: Test f+1 votes from higher round triggers SkipRound
    let ([addr1, addr2, addr3, ..], mut keeper) = setup([1, 1, 1, 1, 1]);

    let id = ValueId::new(1);
    let val = NilOrVal::Val(id);
    let height = Height::new(1);
    let cur_round = Round::new(0);
    let fut_round = Round::new(1);

    // 1 vote in current round
    let vote = new_signed_prevote(height, cur_round, val, addr1);
    assert_eq!(keeper.apply_vote(vote, cur_round), None);

    // 1 vote in future round
    let vote = new_signed_prevote(height, fut_round, val, addr2);
    assert_eq!(keeper.apply_vote(vote, cur_round), None);

    // FaB: 2nd vote from future round reaches f+1 (2 out of 5 = > 1/5)
    let vote = new_signed_prevote(height, fut_round, val, addr3);
    assert_eq!(keeper.apply_vote(vote, cur_round), Some(Output::SkipRound(fut_round)));
}

#[test]
fn fab_no_skip_round_same_validator() {
    // FaB: Same validator voting in different rounds should not trigger skip
    let ([addr1, addr2, ..], mut keeper) = setup([1, 1, 1, 1, 1]);

    let id = ValueId::new(1);
    let val = NilOrVal::Val(id);
    let height = Height::new(1);
    let cur_round = Round::new(0);
    let fut_round = Round::new(1);

    // Vote in current round
    let vote = new_signed_prevote(height, cur_round, val, addr1);
    assert_eq!(keeper.apply_vote(vote, cur_round), None);

    // Same validator votes in future round
    let vote = new_signed_prevote(height, fut_round, val, addr1);
    assert_eq!(keeper.apply_vote(vote, cur_round), None);

    // Different validator in future round
    let vote = new_signed_prevote(height, fut_round, val, addr2);
    // Still only 2 distinct validators, not enough for f+1
    assert_eq!(keeper.apply_vote(vote, cur_round), None);
}

#[test]
fn fab_same_votes_no_equivocation() {
    let ([addr1, ..], mut keeper) = setup([1, 1, 1, 1, 1]);

    let height = Height::new(1);
    let round = Round::new(0);
    let id = ValueId::new(1);
    let val = NilOrVal::Val(id);

    let vote1 = new_signed_prevote(height, round, val, addr1);
    assert_eq!(keeper.apply_vote(vote1.clone(), round), None);

    // Same vote again - should not be treated as equivocation
    let vote2 = new_signed_prevote(height, round, val, addr1);
    assert_eq!(keeper.apply_vote(vote2, round), None);

    assert!(keeper.evidence().is_empty());
    assert_eq!(keeper.evidence().get(&addr1), None);
}

#[test]
fn fab_equivocation_detection() {
    let ([addr1, addr2, ..], mut keeper) = setup([1, 1, 1, 1, 1]);

    let height = Height::new(1);
    let round = Round::new(0);

    let id1 = ValueId::new(1);
    let val1 = NilOrVal::Val(id1);

    // addr1 votes for value1
    let vote11 = new_signed_prevote(height, round, val1, addr1);
    assert_eq!(keeper.apply_vote(vote11.clone(), round), None);

    // addr1 votes for nil (equivocation!)
    let vote12 = new_signed_prevote(height, round, NilOrVal::Nil, addr1);
    assert_eq!(keeper.apply_vote(vote12.clone(), round), None);

    // Evidence should be recorded
    assert!(!keeper.evidence().is_empty());
    assert_eq!(keeper.evidence().get(&addr1), Some(&vec![(vote11, vote12)]));

    // addr2 votes for value1
    let vote21 = new_signed_prevote(height, round, val1, addr2);
    assert_eq!(keeper.apply_vote(vote21.clone(), round), None);

    let id2 = ValueId::new(2);
    let val2 = NilOrVal::Val(id2);

    // addr2 votes for value2 (equivocation!)
    let vote22 = new_signed_prevote(height, round, val2, addr2);
    assert_eq!(keeper.apply_vote(vote22.clone(), round), None);

    // Evidence for addr2 should also be recorded
    assert_eq!(keeper.evidence().get(&addr2), Some(&vec![(vote21, vote22)]));
}
