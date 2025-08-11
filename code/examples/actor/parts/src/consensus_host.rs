use crate::actor::{HostMsg, HostRef};
use crate::types::context::MockContext;
use ractor::{async_trait, Actor, ActorProcessingErr, SpawnErr};

pub struct ConsensusHost {
    host: HostRef,
}

pub type ConsensusHostRef = malachitebft_engine::host::HostRef<MockContext>;
pub type ConsensusHostMsg = malachitebft_engine::host::HostMsg<MockContext>;

#[async_trait]
impl Actor for ConsensusHost {
    type Arguments = ();
    type State = ();
    type Msg = ConsensusHostMsg;

    async fn pre_start(
        &self,
        _myself: ConsensusHostRef,
        _args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle(
        &self,
        _myself: ConsensusHostRef,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let host_msg = HostMsg::Consensus(msg);
        self.host
            .cast(host_msg)
            .map_err(|e| ActorProcessingErr::from(e.to_string()))
    }
}

impl ConsensusHost {
    pub async fn spawn(host: HostRef) -> Result<ConsensusHostRef, SpawnErr> {
        let (actor_ref, _) = Actor::spawn(None, Self::new(host), ()).await?;

        Ok(actor_ref)
    }

    pub fn new(host: HostRef) -> Self {
        Self { host }
    }
}
