use std::sync::Arc;

use async_trait::async_trait;
use ractor::{Actor, ActorRef};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use malachite_common::{Context, Round};
use malachite_driver::ProposerSelector;
use malachite_node::value_builder::ValueBuilder;
use malachite_proto::Protobuf;
use malachite_vote::ThresholdParams;

use crate::consensus::{Consensus, Msg as ConsensusMsg, Params as ConsensusParams};
use crate::gossip::{Gossip, Msg as GossipMsg};
use crate::proposal_builder::ProposalBuilder;
use crate::timers::Config as TimersConfig;

pub struct Params<Ctx: Context> {
    pub address: Ctx::Address,
    pub initial_validator_set: Ctx::ValidatorSet,
    pub keypair: malachite_gossip::Keypair,
    pub proposer_selector: Arc<dyn ProposerSelector<Ctx>>,
    pub start_height: Ctx::Height,
    pub threshold_params: ThresholdParams,
    pub timers_config: TimersConfig,
    pub value_builder: Box<dyn ValueBuilder<Ctx>>,
    pub tx_decision: mpsc::Sender<(Ctx::Height, Round, Ctx::Value)>,
}

pub async fn spawn<Ctx>(
    ctx: Ctx,
    params: Params<Ctx>,
) -> Result<(ActorRef<Msg>, JoinHandle<()>), ractor::ActorProcessingErr>
where
    Ctx: Context,
    Ctx::Vote: Protobuf<Proto = malachite_proto::Vote>,
    Ctx::Proposal: Protobuf<Proto = malachite_proto::Proposal>,
{
    let proposal_builder = ProposalBuilder::spawn(params.value_builder, None).await?;

    let consensus_params = ConsensusParams {
        start_height: params.start_height,
        proposer_selector: params.proposer_selector,
        validator_set: params.initial_validator_set,
        address: params.address,
        threshold_params: params.threshold_params,
    };

    let addr = "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap();
    let config = malachite_gossip::Config::default();
    let gossip = Gossip::spawn(params.keypair, addr, config, None)
        .await
        .unwrap();

    let consensus = Consensus::spawn(
        ctx.clone(),
        consensus_params,
        params.timers_config,
        gossip.clone(),
        proposal_builder,
        params.tx_decision,
        None,
    )
    .await?;

    let node = Node::new(ctx, gossip, consensus, params.start_height);
    let actor = node.spawn().await?;
    Ok(actor)
}

pub struct Node<Ctx: Context> {
    #[allow(dead_code)]
    ctx: Ctx,
    gossip: ActorRef<GossipMsg>,
    consensus: ActorRef<ConsensusMsg<Ctx>>,
    start_height: Ctx::Height,
}

impl<Ctx> Node<Ctx>
where
    Ctx: Context,
    Ctx::Vote: Protobuf<Proto = malachite_proto::Vote>,
    Ctx::Proposal: Protobuf<Proto = malachite_proto::Proposal>,
{
    pub fn new(
        ctx: Ctx,
        gossip: ActorRef<GossipMsg>,
        consensus: ActorRef<ConsensusMsg<Ctx>>,
        start_height: Ctx::Height,
    ) -> Self {
        Self {
            ctx,
            gossip,
            consensus,
            start_height,
        }
    }

    pub async fn spawn(self) -> Result<(ActorRef<Msg>, JoinHandle<()>), ractor::SpawnErr> {
        Actor::spawn(None, self, ()).await
    }
}

pub enum Msg {
    Start,
}

#[async_trait]
impl<Ctx> Actor for Node<Ctx>
where
    Ctx: Context,
    Ctx::Vote: Protobuf<Proto = malachite_proto::Vote>,
    Ctx::Proposal: Protobuf<Proto = malachite_proto::Proposal>,
{
    type Msg = Msg;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        _args: (),
    ) -> Result<(), ractor::ActorProcessingErr> {
        // Set ourselves as the supervisor of the gossip and consensus actors
        self.gossip.link(myself.get_cell());
        self.consensus.link(myself.get_cell());

        Ok(())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut (),
    ) -> Result<(), ractor::ActorProcessingErr> {
        match msg {
            Msg::Start => self
                .consensus
                .cast(crate::consensus::Msg::StartHeight(self.start_height))?,
        }

        Ok(())
    }
}
