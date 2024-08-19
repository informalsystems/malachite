use core::fmt::Debug;

use crate::{Context, Round};

/// Defines the requirements for a proposal part type.
pub trait ProposalPart<Ctx>
where
    Self: Clone + Debug + Eq + Send + Sync + 'static,
    Ctx: Context,
{
    /// The part height
    fn height(&self) -> Ctx::Height;

    /// The part round
    fn round(&self) -> Round;

    /// The part sequence
    fn sequence(&self) -> u64;

    /// Address of the validator who created this part
    fn validator_address(&self) -> &Ctx::Address;
}
