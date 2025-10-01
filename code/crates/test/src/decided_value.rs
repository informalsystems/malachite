use crate::{TestContext, Value};
use malachitebft_core_types::CommitCertificate;

#[derive(Clone, Debug)]
pub struct DecidedValue {
    pub value: Value,
    pub certificate: CommitCertificate<TestContext>,
}
