use derive_where::derive_where;
use tokio::sync::oneshot;

use malachitebft_core_consensus::types::*;

pub type Reply<A> = oneshot::Sender<A>;

#[must_use]
#[derive_where(Debug)]
pub enum Query<Ctx>
where
    Ctx: Context,
{
    Consensus(ConsensusQuery<Ctx>),
    Sync(SyncQuery<Ctx>),
    Wal(WalQuery<Ctx>),
    // Timers(TimersQuery),
    // Signing(SigningQuery<Ctx>),
}

impl<Ctx: Context> From<ConsensusQuery<Ctx>> for Query<Ctx> {
    fn from(query: ConsensusQuery<Ctx>) -> Self {
        Self::Consensus(query)
    }
}

impl<Ctx: Context> From<SyncQuery<Ctx>> for Query<Ctx> {
    fn from(query: SyncQuery<Ctx>) -> Self {
        Self::Sync(query)
    }
}

impl<Ctx: Context> From<WalQuery<Ctx>> for Query<Ctx> {
    fn from(query: WalQuery<Ctx>) -> Self {
        Self::Wal(query)
    }
}

// impl<Ctx: Context> From<TimersQuery> for Query<Ctx> {
//     fn from(query: TimersQuery) -> Self {
//         Self::Timers(query)
//     }
// }
//
// impl<Ctx: Context> From<SigningQuery<Ctx>> for Query<Ctx> {
//     fn from(query: SigningQuery<Ctx>) -> Self {
//         Self::Signing(query)
//     }
// }

#[must_use]
#[derive_where(Debug)]
pub enum ConsensusQuery<Ctx>
where
    Ctx: Context,
{
    /// Consensus is starting a new round with the given proposer
    StartRound(Ctx::Height, Round, Ctx::Address, Reply<()>),

    /// Get the validator set at the given height
    GetValidatorSet(Ctx::Height, Reply<Option<Ctx::ValidatorSet>>),

    /// Publish a message to peers
    Publish(SignedConsensusMsg<Ctx>, Reply<()>),

    /// Requests the application to build a value for consensus to run on.
    ///
    /// Because this operation may be asynchronous, this effect does not expect a resumption
    /// with a value, rather the application is expected to propose a value within the timeout duration.
    ///
    /// The application MUST eventually feed a [`Propose`][crate::Input::Propose]
    /// input to consensus within the specified timeout duration.
    GetValue(Ctx::Height, Round, Timeout, Reply<()>),

    /// Requests the application to re-stream a proposal that it has already seen.
    ///
    /// The application MUST re-publish again to its pwers all
    /// the proposal parts pertaining to that value.
    RestreamValue(
        /// Height of the value
        Ctx::Height,
        /// Round of the value
        Round,
        /// Valid round of the value
        Round,
        /// Address of the proposer for that value
        Ctx::Address,
        /// Value ID of the value to restream
        ValueId<Ctx>,
        /// For resumption
        Reply<()>,
    ),

    /// Notifies the application that consensus has decided on a value.
    ///
    /// This message includes a commit certificate containing the ID of
    /// the value that was decided on, the height and round at which it was decided,
    /// and the aggregated signatures of the validators that committed to it.
    Decide(CommitCertificate<Ctx>, Reply<()>),
}

#[must_use]
#[derive_where(Debug)]
pub enum SyncQuery<Ctx>
where
    Ctx: Context,
{
    /// Consensus has been stuck in Prevote or Precommit step, ask for vote sets from peers
    GetVoteSet(Ctx::Height, Round, Reply<()>),

    /// A peer has required our vote set, send the response
    SendVoteSetResponse(RequestId, Ctx::Height, Round, VoteSet<Ctx>, Reply<()>),
}

#[must_use]
#[derive_where(Debug)]
pub enum WalQuery<Ctx>
where
    Ctx: Context,
{
    /// Append a consensus message to the Write-Ahead Log for crash recovery
    AppendMessage(SignedConsensusMsg<Ctx>, Reply<()>),

    /// Append a timeout to the Write-Ahead Log for crash recovery
    AppendTimeout(Timeout, Reply<()>),
}

// #[must_use]
// #[derive(Debug)]
// pub enum TimersQuery {
//     /// Reset all timeouts to their initial values
//     ResetTimeouts(Reply<()>),
//
//     /// Cancel all outstanding timeouts
//     CancelAllTimeouts(Reply<()>),
//
//     /// Cancel a given timeout
//     CancelTimeout(Timeout, Reply<()>),
//
//     /// Schedule a timeout
//     ScheduleTimeout(Timeout, Reply<()>),
// }

// #[must_use]
// #[derive_where(Debug)]
// pub enum SigningQuery<Ctx>
// where
//     Ctx: Context,
// {
//     /// Sign a vote with this node's private key
//     SignVote(Ctx::Vote, Reply<SignedVote<Ctx>>),
//
//     /// Sign a proposal with this node's private key
//     SignProposal(Ctx::Proposal, Reply<SignedProposal<Ctx>>),
//
//     /// Verify a signature
//     VerifySignature(
//         SignedMessage<Ctx, ConsensusMsg<Ctx>>,
//         PublicKey<Ctx>,
//         Reply<Validity>,
//     ),
//
//     /// Verify a commit certificate
//     VerifyCertificate(
//         CommitCertificate<Ctx>,
//         Ctx::ValidatorSet,
//         ThresholdParams,
//         Reply<Result<(), CertificateError<Ctx>>>,
//     ),
// }
