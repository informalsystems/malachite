use core::fmt::Debug;

use malachite_proto::Protobuf;

use crate::{Context, Round};

/// Defines the requirements for a proposal type.
pub trait Proposal<Ctx>
where
    Self: Clone + Debug + Eq + Send + Sync + 'static,
    Self: Protobuf<Proto = malachite_proto::Proposal>,
    Ctx: Context,
{
    /// The height for which the proposal is for.
    fn height(&self) -> Ctx::Height;

    /// The round for which the proposal is for.
    fn round(&self) -> Round;

    /// The value that is proposed.
    fn value(&self) -> &Ctx::Value;

    /// The POL round for which the proposal is for.
    fn pol_round(&self) -> Round;
}
