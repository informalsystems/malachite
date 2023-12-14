use std::collections::HashMap;

use malachite_common::{Context, Round};
use malachite_itf::consensus::{Input as ModelInput, Output as ModelOutput, State};
use malachite_itf::types::Step;
use malachite_round::input::Input;
use malachite_round::output::Output;
use malachite_round::{state::State as RoundState, state_machine::Info};
use malachite_test::{Address, Height, TestContext};

use itf::Runner as ItfRunner;

use crate::utils::{value_from_model, value_from_string, value_id_from_model};

pub struct ConsensusRunner {
    pub address_map: HashMap<String, Address>,
}

impl ItfRunner for ConsensusRunner {
    type ActualState = RoundState<TestContext>;
    type Result = Option<Output<TestContext>>;
    type ExpectedState = State;
    type Error = ();

    fn init(&mut self, expected: &Self::ExpectedState) -> Result<Self::ActualState, Self::Error> {
        println!("🔵 init: expected_state: {:?}", expected.state);
        let height = expected.state.height;
        let round = expected.state.round as i64;
        let round = if round < 0 { 0 } else { round }; // CHECK: this is a hack, needed because spec starts with round = -1
        println!("🔵 init: height={:?}, round={:?}", height, round);
        let init_state = RoundState::new(Height::new(height as u64), Round::new(round));
        Ok(init_state)
    }

    fn step(
        &mut self,
        actual: &mut Self::ActualState,
        expected: &Self::ExpectedState,
    ) -> Result<Self::Result, Self::Error> {
        println!("🔸 step: actual state={:?}", actual);
        println!("🔸 step: model input={:?}", expected);
        let address = self.address_map.get(expected.state.p.as_str()).unwrap();
        let some_other_node = self.address_map.get("Other").unwrap(); // FIXME
        let (data, input) = match &expected.input {
            ModelInput::NoInput => unreachable!(),
            ModelInput::NewRound(round) => (
                Info::new(Round::new(*round as i64), address, some_other_node),
                Input::NewRound,
            ),
            ModelInput::NewRoundProposer(round, _value) => {
                // TODO: proposal value not used?
                (
                    Info::new(Round::new(*round as i64), address, address),
                    Input::NewRound,
                )
            }
            ModelInput::Proposal(round, value) => {
                let input_round = Round::new(*round as i64);
                let data = Info::new(input_round, address, some_other_node);
                let proposal = TestContext::new_proposal(
                    actual.height,
                    input_round,
                    value_from_model(&value).unwrap(),
                    Round::Nil,
                );
                (data, Input::Proposal(proposal))
            }
            ModelInput::ProposalAndPolkaPreviousAndValid(value, valid_round) => {
                let input_round = Round::new(*valid_round as i64);
                let data = Info::new(input_round, address, some_other_node);
                let proposal = TestContext::new_proposal(
                    actual.height,
                    input_round,
                    value_from_model(&value).unwrap(),
                    Round::new(0), // FIXME
                );
                (data, Input::ProposalAndPolkaPrevious(proposal))
            }
            ModelInput::ProposalAndPolkaAndValid(value) => {
                let data = Info::new(actual.round, address, some_other_node);
                let proposal = TestContext::new_proposal(
                    actual.height,
                    actual.round,
                    value_from_model(&value).unwrap(),
                    Round::new(0), // FIXME
                );
                (data, Input::ProposalAndPolkaCurrent(proposal))
            }
            ModelInput::ProposalAndPolkaAndInvalidCInput(_height, _round, _value) => todo!(),
            ModelInput::ProposalAndCommitAndValid(value) => {
                let data = Info::new(actual.round, address, some_other_node);
                let proposal = TestContext::new_proposal(
                    actual.height,
                    actual.round,
                    value_from_model(&value).unwrap(),
                    Round::new(0), // FIXME
                );
                (data, Input::ProposalAndPrecommitValue(proposal))
            }
            ModelInput::ProposalInvalid => todo!(),
            ModelInput::PolkaNil => (
                Info::new(actual.round, address, some_other_node),
                Input::PolkaNil,
            ),
            ModelInput::PolkaAny => (
                Info::new(actual.round, address, some_other_node),
                Input::PolkaAny,
            ),
            ModelInput::PrecommitAny => (
                Info::new(actual.round, address, some_other_node),
                Input::PrecommitAny,
            ),
            ModelInput::RoundSkip(_round) => todo!(),
            ModelInput::TimeoutPropose(_height, round) => (
                Info::new(Round::new(*round as i64), address, some_other_node),
                Input::TimeoutPropose,
            ),
            ModelInput::TimeoutPrevote(_height, round) => (
                Info::new(Round::new(*round as i64), address, some_other_node),
                Input::TimeoutPrevote,
            ),
            ModelInput::TimeoutPrecommit(_height, round) => (
                Info::new(Round::new(*round as i64), address, some_other_node),
                Input::TimeoutPrecommit,
            ),
        };
        let round_state = core::mem::take(actual);
        let transition = round_state.apply(&data, input);
        println!("🔹 transition: next_state={:?}", transition.next_state);
        println!("🔹 transition: output={:?}", transition.output);
        *actual = transition.next_state;
        Ok(transition.output)
    }

    fn result_invariant(
        &self,
        result: &Self::Result,
        expected: &Self::ExpectedState,
    ) -> Result<bool, Self::Error> {
        // Get expected result.
        let expected_result = &expected.output;
        println!("🟣 result_invariant: actual output={:?}", result);
        println!("🟣 result_invariant: expected output={:?}", expected_result);
        // Check result against expected result.
        match result {
            Some(result) => match (result, expected_result) {
                (Output::NewRound(round), ModelOutput::SkipRound(expected_round)) => {
                    assert_eq!(round.as_i64(), *expected_round);
                }
                (Output::Proposal(proposal), ModelOutput::Proposal(expected_proposal)) => {
                    // TODO: check expected_proposal.src_address
                    assert_eq!(proposal.height.as_u64() as i64, expected_proposal.height);
                    assert_eq!(proposal.round.as_i64(), expected_proposal.round);
                    assert_eq!(proposal.pol_round.as_i64(), expected_proposal.valid_round);
                    assert_eq!(
                        Some(proposal.value),
                        value_from_string(&expected_proposal.proposal)
                    );
                }
                (Output::Vote(vote), ModelOutput::Vote(expected_vote)) => {
                    let expected_src_address = self
                        .address_map
                        .get(expected_vote.src_address.as_str())
                        .unwrap();
                    assert_eq!(vote.validator_address, *expected_src_address);
                    assert_eq!(vote.typ, expected_vote.vote_type.to_common());
                    assert_eq!(vote.height.as_u64() as i64, expected_vote.height);
                    assert_eq!(vote.round.as_i64(), expected_vote.round);
                    // assert_eq!(vote.value, value_id_from_model(&expected_vote.value_id));
                }
                (Output::ScheduleTimeout(timeout), ModelOutput::Timeout(expected_timeout)) => {
                    assert_eq!(timeout.step, expected_timeout.to_common());
                    // CHECK: spec does not have round in timeout
                }
                (
                    Output::GetValueAndScheduleTimeout(_round, _timeout),
                    ModelOutput::Proposal(_),
                ) => {
                    // TODO: Check this case (GetValueAndScheduleTimeout is the output of NewProposal)
                    ()
                }
                (Output::Decision(decision), ModelOutput::Decided(expected_decided_value)) => {
                    assert_eq!(
                        Some(decision.value),
                        value_from_model(&expected_decided_value)
                    );
                }
                _ => panic!("actual: {:?}\nexpected: {:?}", result, expected_result),
            },
            None => panic!("no actual result; expected result: {:?}", expected_result),
        }
        Ok(true)
    }

    fn state_invariant(
        &self,
        actual: &Self::ActualState,
        expected: &Self::ExpectedState,
    ) -> Result<bool, Self::Error> {
        // TODO: What to do with actual.height? There is no height in the spec.

        println!("🟢 state_invariant: actual state={:?}", actual);
        println!("🟢 state_invariant: expected state={:?}", expected.state);

        if expected.state.step == Step::None {
            // This is the initial state.
            // The round in the spec's initial state is -1, while in the code, it's 0.
            assert_eq!(actual.round.as_i64(), 0);
        } else {
            assert_eq!(Some(actual.step), expected.state.step.to_round_step());
            if expected.state.step == Step::NewRound {
                // In the spec, the new round comes from the input, it's not in the state.
                assert_eq!(actual.round.as_i64(), expected.state.round + 1);
            } else {
                assert_eq!(actual.round.as_i64(), expected.state.round);
            }
        }
        assert_eq!(
            actual.valid.as_ref().map(|v| v.round.as_i64()),
            expected.state.valid_round.map(|vr| vr as i64),
        );
        assert_eq!(
            actual.valid.as_ref().map(|v| v.value.id()),
            value_id_from_model(&expected.state.valid_value),
        );
        assert_eq!(
            actual.locked.as_ref().map(|v| v.round.as_i64()),
            expected.state.locked_round.map(|vr| vr as i64),
        );
        assert_eq!(
            actual.locked.as_ref().map(|v| v.value.id()),
            value_id_from_model(&expected.state.locked_value),
        );
        Ok(true)
    }
}
