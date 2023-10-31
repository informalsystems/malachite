use malachite_test::{Address, Height, Proposal, TestConsensus, Value};

use malachite_common::{Round, Timeout, TimeoutStep};
use malachite_round::events::Event;
use malachite_round::message::Message;
use malachite_round::state::{State, Step};
use malachite_round::state_machine::apply_event;

const ADDRESS: Address = Address::new([42; 20]);

#[test]
fn test_propose() {
    let value = Value::new(42);
    let mut state: State<TestConsensus> = State::new(Height::new(10), ADDRESS);

    let transition = apply_event(state.clone(), Round::new(0), Event::NewRoundProposer(value));

    state.step = Step::Propose;
    assert_eq!(transition.next_state, state);

    assert_eq!(
        transition.message.unwrap(),
        Message::proposal(Height::new(10), Round::new(0), Value::new(42), Round::Nil)
    );
}

#[test]
fn test_prevote() {
    let value = Value::new(42);
    let state: State<TestConsensus> = State::new(Height::new(1), ADDRESS).new_round(Round::new(1));

    let transition = apply_event(state, Round::new(1), Event::NewRound);

    assert_eq!(transition.next_state.step, Step::Propose);
    assert_eq!(
        transition.message.unwrap(),
        Message::Timeout(Timeout {
            round: Round::new(1),
            step: TimeoutStep::Propose
        })
    );

    let state = transition.next_state;

    let transition = apply_event(
        state,
        Round::new(1),
        Event::Proposal(Proposal::new(
            Height::new(1),
            Round::new(1),
            value.clone(),
            Round::Nil,
        )),
    );

    assert_eq!(transition.next_state.step, Step::Prevote);
    assert_eq!(
        transition.message.unwrap(),
        Message::prevote(Round::new(1), Some(value.id()), ADDRESS)
    );
}
