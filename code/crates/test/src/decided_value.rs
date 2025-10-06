use crate::{TestContext, Value};
// FaB: Import Certificate from state machine (4f+1 prevote certificate)
// FaB: Remove CommitCertificate (Tendermint 2f+1 precommit concept)
use malachitebft_core_state_machine::input::Certificate;

#[derive(Clone, Debug)]
pub struct DecidedValue {
    pub value: Value,
    /// FaB: Certificate is now a Vec<SignedVote<TestContext>> containing 4f+1 prevotes
    pub certificate: Certificate<TestContext>,
}
