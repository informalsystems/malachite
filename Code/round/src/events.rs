use malachite_common::{Context, ValueId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event<Ctx>
where
    Ctx: Context,
{
    NewRound,                                 // Start a new round, not as proposer.L20
    NewRoundProposer(Ctx::Value),             // Start a new round and propose the Value.L14
    Proposal(Ctx::Proposal),                  // Receive a proposal. L22 + L23 (valid)
    ProposalAndPolkaPrevious(Ctx::Proposal), // Recieved a proposal and a polka value from a previous round. L28 + L29 (valid)
    ProposalInvalid,                         // Receive an invalid proposal. L26 + L32 (invalid)
    PolkaValue(ValueId<Ctx>),                // Receive +2/3 prevotes for valueId. L44
    PolkaAny,                                // Receive +2/3 prevotes for anything. L34
    PolkaNil,                                // Receive +2/3 prevotes for nil. L44
    ProposalAndPolkaCurrent(Ctx::Proposal), // Receive +2/3 prevotes for Value in current round. L36
    PrecommitAny,                           // Receive +2/3 precommits for anything. L47
    ProposalAndPrecommitValue(Ctx::Proposal), // Receive +2/3 precommits for Value. L49
    PrecommitValue(ValueId<Ctx>),           // Receive +2/3 precommits for ValueId. L51
    RoundSkip, // Receive +1/3 messages from a higher round. OneCorrectProcessInHigherRound, L55
    TimeoutPropose, // Timeout waiting for proposal. L57
    TimeoutPrevote, // Timeout waiting for prevotes. L61
    TimeoutPrecommit, // Timeout waiting for precommits. L65
}
