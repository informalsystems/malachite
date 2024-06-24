use async_trait::async_trait;
use ractor::{Actor, ActorProcessingErr, ActorRef, SupervisionEvent};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, error, warn};

use malachite_common::{Context, Round};
use malachite_proto::Protobuf;
use malachite_vote::ThresholdParams;

use crate::consensus::ConsensusRef;
use crate::gossip_consensus::GossipConsensusRef;
use crate::gossip_mempool::GossipMempoolRef;
use crate::host::HostRef;
use crate::mempool::MempoolRef;
use crate::timers::Config as TimersConfig;

pub type NodeRef = ActorRef<()>;

pub struct Params<Ctx: Context> {
    pub address: Ctx::Address,
    pub initial_validator_set: Ctx::ValidatorSet,
    pub keypair: malachite_gossip_consensus::Keypair,
    pub start_height: Ctx::Height,
    pub threshold_params: ThresholdParams,
    pub timers_config: TimersConfig,
    pub gossip_mempool: GossipMempoolRef,
    pub mempool: MempoolRef,
    pub tx_decision: mpsc::Sender<(Ctx::Height, Round, Ctx::Value)>,
}

#[allow(dead_code)]
pub struct Node<Ctx: Context> {
    ctx: Ctx,
    gossip_consensus: GossipConsensusRef,
    consensus: ConsensusRef<Ctx>,
    gossip_mempool: GossipMempoolRef,
    mempool: MempoolRef,
    host: HostRef<Ctx>,
    start_height: Ctx::Height,
}

impl<Ctx> Node<Ctx>
where
    Ctx: Context,
    Ctx::Vote: Protobuf<Proto = malachite_proto::Vote>,
    Ctx::Proposal: Protobuf<Proto = malachite_proto::Proposal>,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ctx: Ctx,
        gossip_consensus: GossipConsensusRef,
        consensus: ConsensusRef<Ctx>,
        gossip_mempool: GossipMempoolRef,
        mempool: MempoolRef,
        host: HostRef<Ctx>,
        start_height: Ctx::Height,
    ) -> Self {
        Self {
            ctx,
            gossip_consensus,
            consensus,
            gossip_mempool,
            mempool,
            host,
            start_height,
        }
    }

    pub async fn spawn(self) -> Result<(ActorRef<()>, JoinHandle<()>), ractor::SpawnErr> {
        Actor::spawn(None, self, ()).await
    }
}

#[async_trait]
impl<Ctx> Actor for Node<Ctx>
where
    Ctx: Context,
    Ctx::Vote: Protobuf<Proto = malachite_proto::Vote>,
    Ctx::Proposal: Protobuf<Proto = malachite_proto::Proposal>,
{
    type Msg = ();
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        _args: (),
    ) -> Result<(), ActorProcessingErr> {
        // Set ourselves as the supervisor of the other actors
        self.gossip_consensus.link(myself.get_cell());
        self.consensus.link(myself.get_cell());
        self.gossip_mempool.link(myself.get_cell());
        self.mempool.link(myself.get_cell());
        self.host.link(myself.get_cell());

        Ok(())
    }

    #[tracing::instrument(name = "node", skip_all)]
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        _msg: Self::Msg,
        _state: &mut (),
    ) -> Result<(), ActorProcessingErr> {
        Ok(())
    }

    #[tracing::instrument(name = "node", skip_all)]
    async fn handle_supervisor_evt(
        &self,
        _myself: ActorRef<Self::Msg>,
        evt: SupervisionEvent,
        _state: &mut (),
    ) -> Result<(), ActorProcessingErr> {
        match evt {
            SupervisionEvent::ActorStarted(cell) => {
                info!("Actor {} has started", cell.get_id());
            }
            SupervisionEvent::ActorTerminated(cell, _state, reason) => {
                warn!(
                    "Actor {} has terminated: {}",
                    cell.get_id(),
                    reason.unwrap_or_default()
                );
            }
            SupervisionEvent::ActorFailed(cell, error) => {
                error!("Actor {} has failed: {error}", cell.get_id());
            }
            SupervisionEvent::ProcessGroupChanged(_) => (),
        }

        Ok(())
    }
}
