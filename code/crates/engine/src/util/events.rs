use core::fmt;
use std::sync::Arc;

use derive_where::derive_where;
use ractor::ActorProcessingErr;
use tokio::sync::broadcast;

use malachitebft_core_consensus::{
    LocallyProposedValue, ProposedValue, Role, SignedConsensusMsg, WalEntry,
};
// FaB: Import Certificate from state machine (4f+1 prevote certificate)
// FaB: Remove CommitCertificate and PolkaCertificate (Tendermint concepts)
use malachitebft_core_state_machine::input::Certificate;
use malachitebft_core_types::{Context, Round, RoundCertificate, SignedVote, ValueOrigin};

pub type RxEvent<Ctx> = broadcast::Receiver<Event<Ctx>>;

#[derive_where(Clone)]
pub struct TxEvent<Ctx: Context> {
    tx: broadcast::Sender<Event<Ctx>>,
}

impl<Ctx: Context> TxEvent<Ctx> {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(128);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event<Ctx>> {
        self.tx.subscribe()
    }

    pub fn send(&self, event: impl FnOnce() -> Event<Ctx>) {
        if self.tx.receiver_count() > 0 {
            let _ = self.tx.send(event());
        }
    }
}

impl<Ctx: Context> Default for TxEvent<Ctx> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive_where(Clone, Debug)]
pub enum Event<Ctx: Context> {
    StartedHeight(Ctx::Height, bool),
    StartedRound(Ctx::Height, Round, Ctx::Address, Role),
    Published(SignedConsensusMsg<Ctx>),
    ProposedValue(LocallyProposedValue<Ctx>),
    ReceivedProposedValue(ProposedValue<Ctx>, ValueOrigin),
    // FaB: Changed from CommitCertificate to Certificate (4f+1 prevote certificate)
    Decided(Certificate<Ctx>),
    RepublishVote(SignedVote<Ctx>),
    RebroadcastRoundCertificate(RoundCertificate<Ctx>),
    SkipRoundCertificate(RoundCertificate<Ctx>),
    // FaB: Removed PolkaCertificate - Tendermint 2f+1 prevote concept not used in FaB
    WalReplayBegin(Ctx::Height, usize),
    WalReplayEntry(WalEntry<Ctx>),
    WalReplayDone(Ctx::Height),
    WalReplayError(Arc<ActorProcessingErr>),
}

impl<Ctx: Context> fmt::Display for Event<Ctx> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::StartedHeight(height, restart) => {
                write!(f, "StartedHeight(height: {height}, restart: {restart})")
            }
            Event::StartedRound(height, round, proposer, role) => {
                write!(f, "StartedRound(height: {height}, round: {round}, proposer: {proposer}, role: {role:?})")
            }
            Event::Published(msg) => write!(f, "Published(msg: {msg:?})"),
            Event::ProposedValue(value) => write!(f, "ProposedValue(value: {value:?})"),
            Event::ReceivedProposedValue(value, origin) => {
                write!(
                    f,
                    "ReceivedProposedValue(value: {value:?}, origin: {origin:?})"
                )
            }
            // FaB: Certificate is a Vec<SignedVote<Ctx>>, display count instead
            Event::Decided(cert) => write!(f, "Decided({} votes)", cert.len()),
            Event::RepublishVote(vote) => write!(f, "RepublishVote(vote: {vote:?})"),
            Event::RebroadcastRoundCertificate(certificate) => write!(
                f,
                "RebroadcastRoundCertificate(certificate: {certificate:?})"
            ),
            Event::WalReplayBegin(height, count) => {
                write!(f, "WalReplayBegin(height: {height}, count: {count})")
            }
            Event::WalReplayEntry(entry) => write!(f, "WalReplayEntry(entry: {entry:?})"),
            Event::WalReplayDone(height) => write!(f, "WalReplayDone(height: {height})"),
            Event::WalReplayError(error) => write!(f, "WalReplayError({error})"),
            // FaB: Removed PolkaCertificate case
            Event::SkipRoundCertificate(certificate) => {
                write!(f, "SkipRoundCertificate: {certificate:?})")
            }
        }
    }
}
