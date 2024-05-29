use malachite_test::{Address, Height, Proposal, TestContext, Value};

use malachite_common::{NilOrVal, Round, Timeout, TimeoutStep};
use malachite_round::input::Input;
use malachite_round::output::Output;
use malachite_round::state::{State, Step};
use malachite_round::state_machine::{apply, Info};

const ADDRESS: Address = Address::new([42; 20]);
const OTHER_ADDRESS: Address = Address::new([21; 20]);

#[test]
fn test_propose() {
    let value = Value::new(42);
    let height = Height::new(10);
    let round = Round::new(0);

    let mut state: State<TestContext> = State {
        height,
        round,
        ..Default::default()
    };

    // We are the proposer
    let data = Info::new(round, &ADDRESS, &ADDRESS);

    let transition = apply(state.clone(), &data, Input::NewRound(round));

    state.step = Step::Propose;
    assert_eq!(transition.next_state, state);
    assert_eq!(
        transition.output.unwrap(),
        Output::get_value_and_schedule_timeout(height, round, TimeoutStep::Propose)
    );

    let transition = apply(transition.next_state, &data, Input::ProposeValue(value));

    state.step = Step::Propose;
    assert_eq!(transition.next_state, state);
    assert_eq!(
        transition.output.unwrap(),
        Output::proposal(
            Height::new(10),
            Round::new(0),
            Value::new(42),
            Round::Nil,
            ADDRESS
        )
    );
}

#[test]
fn test_prevote() {
    let value = Value::new(42);
    let height = Height::new(1);
    let round = Round::new(1);

    let state: State<TestContext> = State {
        height,
        round,
        ..Default::default()
    };

    // We are not the proposer
    let data = Info::new(round, &ADDRESS, &OTHER_ADDRESS);

    let transition = apply(state, &data, Input::NewRound(round));

    assert_eq!(transition.next_state.step, Step::Propose);
    assert_eq!(
        transition.output.unwrap(),
        Output::ScheduleTimeout(Timeout {
            round: Round::new(1),
            step: TimeoutStep::Propose
        })
    );

    let state = transition.next_state;

    let transition = apply(
        state,
        &data,
        Input::Proposal(Proposal::new(
            Height::new(1),
            Round::new(1),
            value,
            Round::Nil,
            OTHER_ADDRESS,
        )),
    );

    assert_eq!(transition.next_state.step, Step::Prevote);
    assert_eq!(
        transition.output.unwrap(),
        Output::prevote(
            Height::new(1),
            Round::new(1),
            NilOrVal::Val(value.id()),
            ADDRESS
        )
    );
}

#[test]
fn test_input_message_while_commit_step() {
    let value = Value::new(42);
    let height = Height::new(1);
    let round = Round::new(1);

    let state: State<TestContext> = State {
        height,
        round,
        ..Default::default()
    };

    let proposal = Proposal::new(
        Height::new(1),
        Round::new(1),
        value,
        Round::Nil,
        OTHER_ADDRESS,
    );

    let data = Info::new(round, &ADDRESS, &OTHER_ADDRESS);

    let mut transition = apply(state, &data, Input::NewRound(round));
    let mut state = transition.next_state;

    // Go to Commit step via L49
    transition = apply(
        state,
        &data,
        Input::ProposalAndPrecommitValue(proposal.clone()),
    );
    state = transition.next_state;
    assert_eq!(state.step, Step::Commit);

    // Send a proposal message while in Commit step, transition should be invalid
    transition = apply(state, &data, Input::Proposal(proposal));
    state = transition.next_state;

    assert_eq!(state.step, Step::Commit);
    assert!(!transition.valid);
}
