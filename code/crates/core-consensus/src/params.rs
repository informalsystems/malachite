use derive_where::derive_where;

use malachitebft_core_types::{Context, ValuePayload};

// FaB: Removed HIDDEN_LOCK_ROUND - Tendermint 3f+1 liveness mechanism not used in FaB

#[doc(inline)]
pub use malachitebft_core_driver::ThresholdParams;

/// Consensus parameters.
#[derive_where(Clone, Debug)]
pub struct Params<Ctx: Context> {
    /// The initial height
    pub initial_height: Ctx::Height,

    /// The initial validator set
    pub initial_validator_set: Ctx::ValidatorSet,

    /// The address of this validator
    pub address: Ctx::Address,

    /// The quorum and honest thresholds
    pub threshold_params: ThresholdParams,

    /// The messages required to deliver proposals
    pub value_payload: ValuePayload,
}
