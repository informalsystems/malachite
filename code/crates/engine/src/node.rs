use async_trait::async_trait;
use ractor::{Actor, ActorProcessingErr, ActorRef, SupervisionEvent};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use malachitebft_core_types::Context;

use crate::consensus::ConsensusRef;
use crate::host::HostRef;
use crate::network::NetworkRef;
use crate::sync::SyncRef;
use crate::wal::WalRef;

pub type NodeRef = ActorRef<()>;

#[allow(dead_code)]
pub struct Node<Ctx: Context> {
    ctx: Ctx,
    network: NetworkRef<Ctx>,
    consensus: ConsensusRef<Ctx>,
    wal: WalRef<Ctx>,
    sync: Option<SyncRef<Ctx>>,
    host: HostRef<Ctx>,
    span: tracing::Span,
}

impl<Ctx> Node<Ctx>
where
    Ctx: Context,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ctx: Ctx,
        network: NetworkRef<Ctx>,
        consensus: ConsensusRef<Ctx>,
        wal: WalRef<Ctx>,
        sync: Option<SyncRef<Ctx>>,
        host: HostRef<Ctx>,
        span: tracing::Span,
    ) -> Self {
        Self {
            ctx,
            network,
            consensus,
            wal,
            sync,
            host,
            span,
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
        self.network.link(myself.get_cell());
        self.consensus.link(myself.get_cell());
        self.host.link(myself.get_cell());
        self.wal.link(myself.get_cell());

        if let Some(actor) = &self.sync {
            actor.link(myself.get_cell());
        }

        Ok(())
    }

    #[tracing::instrument(name = "node", parent = &self.span, skip_all)]
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        _msg: Self::Msg,
        _state: &mut (),
    ) -> Result<(), ActorProcessingErr> {
        Ok(())
    }

    #[tracing::instrument(name = "node", parent = &self.span, skip_all)]
    async fn handle_supervisor_evt(
        &self,
        _myself: ActorRef<Self::Msg>,
        evt: SupervisionEvent,
        _state: &mut (),
    ) -> Result<(), ActorProcessingErr> {
        match evt {
            SupervisionEvent::ActorStarted(cell) => {
                info!(actor = %cell.get_id(), "Actor has started");
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
