use eyre::{eyre, Result};

use malachite_consensus::{
    process, ConsensusMsg, Context, Metrics, Params, Round, SignedConsensusMsg, ThresholdParams,
    Timeout, ValuePayload, ValueToPropose,
};
use malachite_core_types::{NilOrVal, SignedMessage};
use malachite_test::utils::validators::make_validators;
use malachite_test::{Address, Height, Proposal, TestContext, ValidatorSet, Value, Vote};

type State = malachite_consensus::State<TestContext>;
type Resume = malachite_consensus::Resume<TestContext>;
type Effect = malachite_consensus::Effect<TestContext>;
type Input = malachite_consensus::Input<TestContext>;

#[test]
fn start_height_proposer() -> Result<()> {
    do_start_height(Height::new(1), true)?;
    Ok(())
}

#[test]
fn start_height_non_proposer() -> Result<()> {
    do_start_height(Height::new(1), false)?;
    Ok(())
}

#[test]
fn propose() -> Result<()> {
    let round = Round::new(0);
    let height = Height::new(1);

    let (mut state, ctx, metrics) = do_start_height(height, true)?;

    let validator_set = state.validator_set();
    let proposer = validator_set.get_by_index(0).unwrap();
    let public_key = proposer.public_key;

    let value = Value::new(64);

    let proposal = Proposal::new(height, round, value, Round::Nil, *state.address());
    let signed_proposal = ctx.sign_proposal(proposal.clone());

    let vote = Vote::new_prevote(height, round, NilOrVal::Val(value.id()), proposer.address);
    let signed_vote = ctx.sign_vote(vote.clone());

    let mut handle_effect = expect_effects(vec![
        (
            Effect::CancelTimeout(Timeout::propose(round)),
            Resume::Continue,
        ),
        (
            Effect::SignProposal(proposal.clone()),
            Resume::SignedProposal(signed_proposal.clone()),
        ),
        (
            Effect::VerifySignature(
                SignedMessage::new(
                    ConsensusMsg::Proposal(proposal.clone()),
                    signed_proposal.signature,
                ),
                public_key,
            ),
            Resume::SignatureValidity(true),
        ),
        (
            Effect::PersistMessage(SignedConsensusMsg::Proposal(SignedMessage::new(
                proposal,
                signed_proposal.signature,
            ))),
            Resume::Continue,
        ),
        (
            Effect::CancelTimeout(Timeout::propose(round)),
            Resume::Continue,
        ),
        (
            Effect::ScheduleTimeout(Timeout::prevote_time_limit(round)),
            Resume::Continue,
        ),
        (
            Effect::SignVote(vote.clone()),
            Resume::SignedVote(signed_vote.clone()),
        ),
        (
            Effect::VerifySignature(
                SignedMessage::new(ConsensusMsg::Vote(vote.clone()), signed_vote.signature),
                public_key,
            ),
            Resume::SignatureValidity(true),
        ),
        (
            Effect::PersistMessage(SignedConsensusMsg::Vote(SignedMessage::new(
                vote,
                signed_vote.signature,
            ))),
            Resume::Continue,
        ),
        (
            Effect::Broadcast(SignedConsensusMsg::Vote(signed_vote)),
            Resume::Continue,
        ),
        (
            Effect::Broadcast(SignedConsensusMsg::Proposal(signed_proposal)),
            Resume::Continue,
        ),
    ]);

    let value_to_propose = ValueToPropose {
        height,
        round,
        valid_round: Round::Nil,
        value,
        extension: None,
    };

    process!(
        input: Input::Propose(value_to_propose),
        state: &mut state,
        metrics: &metrics,
        with: effect => handle_effect(effect)
    )
}

#[test]
fn timeout_elapsed_propose() -> Result<()> {
    let height = Height::new(1);
    let (mut state, _ctx, metrics) = setup(height, true);

    let mut handle_effect = expect_effects(vec![]);

    process!(
        input: Input::TimeoutElapsed(Timeout::propose(Round::new(0))),
        state: &mut state,
        metrics: &metrics,
        with: effect => handle_effect(effect)
    )
}

#[test]
fn timeout_elapsed_prevote() -> Result<()> {
    let height = Height::new(1);
    let (mut state, _ctx, metrics) = setup(height, true);

    let mut handle_effect = expect_effects(vec![]);

    process!(
        input: Input::TimeoutElapsed(Timeout::prevote(Round::new(0))),
        state: &mut state,
        metrics: &metrics,
        with: effect => handle_effect(effect)
    )
}

#[test]
fn timeout_elapsed_precommit() -> Result<()> {
    let height = Height::new(1);
    let (mut state, _ctx, metrics) = setup(height, true);

    let mut handle_effect = expect_effects(vec![]);

    process!(
        input: Input::TimeoutElapsed(Timeout::precommit(Round::new(0))),
        state: &mut state,
        metrics: &metrics,
        with: effect => handle_effect(effect)
    )
}

// #[test]
// fn timeout_elapsed_commit() -> Result<()> {
//     let height = Height::new(1);
//     let (mut state, _ctx, metrics) = setup(height, false);
//
//     let mut handle_effect = expect_effects(vec![]);
//
//     process!(
//         input: Input::TimeoutElapsed(Timeout::commit(Round::new(0))),
//         state: &mut state,
//         metrics: &metrics,
//         with: effect => handle_effect(effect)
//     )
// }

fn do_start_height(height: Height, is_proposer: bool) -> Result<(State, TestContext, Metrics)> {
    let (mut state, ctx, metrics) = setup(height, is_proposer);

    let validator_set = state.validator_set().clone();
    let proposer = validator_set.get_by_index(0).unwrap().address;

    let mut expected = vec![
        (Effect::CancelAllTimeouts, Resume::Continue),
        (Effect::ResetTimeouts, Resume::Continue),
        (Effect::CancelAllTimeouts, Resume::Continue),
        (
            Effect::StartRound(height, Round::new(0), proposer),
            Resume::Continue,
        ),
        (
            Effect::ScheduleTimeout(Timeout::propose(Round::new(0))),
            Resume::Continue,
        ),
    ];

    if is_proposer {
        expected.push((
            Effect::GetValue(height, Round::new(0), Timeout::propose(Round::new(0))),
            Resume::Continue,
        ));
    }

    let mut handle_effect = expect_effects(expected);

    let result: Result<()> = process!(
        input: Input::StartHeight(height, validator_set),
        state: &mut state,
        metrics: &metrics,
        with: effect => handle_effect(effect)
    );

    result?;

    Ok((state, ctx, metrics))
}

fn setup(height: Height, is_proposer: bool) -> (State, TestContext, Metrics) {
    let (validators, private_keys) = make_validators([1, 1, 1])
        .into_iter()
        .unzip::<_, _, Vec<_>, Vec<_>>();

    let private_key = if is_proposer {
        private_keys[0].clone()
    } else {
        private_keys[1].clone()
    };

    let address = Address::from_public_key(&private_key.public_key());
    let validator_set = ValidatorSet::new(validators);

    let ctx = TestContext::new(private_key);

    let params = Params {
        start_height: height,
        initial_validator_set: validator_set.clone(),
        address,
        threshold_params: ThresholdParams::default(),
        value_payload: ValuePayload::ProposalAndParts,
    };

    let state = State::new(ctx.clone(), params);
    let metrics = Metrics::default();

    (state, ctx, metrics)
}

fn expect_effects(expected: Vec<(Effect, Resume)>) -> impl FnMut(Effect) -> Result<Resume> {
    let mut expected = expected.into_iter();

    move |effect: Effect| match expected.next() {
        Some((expected, resume)) if expected == effect => Ok(resume),

        Some((expected, _)) => Err(eyre!(
            "unexpected effect: got {effect:?}, expected {expected:?}"
        )),

        None => Err(eyre!("unexpected effect: {effect:?}")),
    }
}
