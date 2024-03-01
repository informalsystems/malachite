use std::fmt::Display;
use std::sync::Arc;
use std::time::{Duration, Instant};

use driver::Validity;
use malachite_common::{
    Context, Height, NilOrVal, Proposal, Round, SignedProposal, SignedVote, Timeout, TimeoutStep,
    Validator, ValidatorSet, Vote, VoteType,
};
use malachite_driver as driver;
use malachite_driver::Driver;
use malachite_gossip::actor::Msg as GossipMsg;
use malachite_gossip::Event as GossipEvent;
use malachite_proto as proto;
use malachite_proto::Protobuf;
use malachite_vote::Threshold;
use ractor::{Actor, ActorName, ActorProcessingErr, ActorRef};
use tracing::{debug, error, info, warn};

use crate::network::Msg as NetworkMsg;
use crate::network::PeerId;
use crate::node::Params;
use crate::timers;
use crate::timers::actor::{Msg as TimerMsg, TimeoutElapsed, Timers};
use crate::util::actor::forward;

use super::Next;

pub struct Node<Ctx>
where
    Ctx: Context,
{
    ctx: Ctx,
    params: Params<Ctx>,
    gossip: ActorRef<GossipMsg>,
    timers_config: timers::Config,
}

pub enum Msg<Ctx: Context> {
    StartHeight(Ctx::Height),
    MoveToNextHeight,
    GossipEvent(Arc<GossipEvent>),
    TimeoutElapsed(Timeout),
    SendDriverInput(driver::Input<Ctx>),
    Decided(Ctx::Height, Round, Ctx::Value),
}

impl<Ctx: Context> From<TimeoutElapsed> for Msg<Ctx> {
    fn from(msg: TimeoutElapsed) -> Self {
        Msg::TimeoutElapsed(msg.timeout())
    }
}

pub struct State<Ctx>
where
    Ctx: Context,
{
    driver: Driver<Ctx>,
    timers: ActorRef<TimerMsg>,
}

pub struct Args;

impl<Ctx> Node<Ctx>
where
    Ctx: Context,
    Ctx::Height: Display,
    Ctx::Vote: Protobuf<Proto = proto::Vote>,
    Ctx::Proposal: Protobuf<Proto = proto::Proposal>,
{
    pub async fn spawn(
        ctx: Ctx,
        params: Params<Ctx>,
        gossip: ActorRef<GossipMsg>,
        timers_config: timers::Config,
    ) -> Result<ActorRef<Msg<Ctx>>, ractor::SpawnErr> {
        let node = Self {
            ctx,
            params,
            gossip,
            timers_config,
        };

        let (actor_ref, _) = Actor::spawn(Some(ActorName::from("Node")), node, Args).await?;

        Ok(actor_ref)
    }

    pub async fn handle_gossip_event(
        &self,
        event: &GossipEvent,
        myself: ActorRef<Msg<Ctx>>,
        state: &mut State<Ctx>,
    ) -> Result<(), ractor::ActorProcessingErr> {
        match event {
            GossipEvent::Listening(addr) => {
                info!("Listening on {addr}");
            }
            GossipEvent::PeerConnected(peer_id) => {
                info!("Connected to peer {peer_id}");
            }
            GossipEvent::PeerDisconnected(peer_id) => {
                info!("Disconnected from peer {peer_id}");
            }
            GossipEvent::Message(from, data) => {
                let from = PeerId::new(from.to_string());
                let msg = NetworkMsg::from_network_bytes(data).unwrap();

                info!("Received message from peer {from}: {msg:?}");

                self.handle_network_msg(from, msg, myself, state).await?;
            }
        }

        Ok(())
    }

    pub async fn handle_network_msg(
        &self,
        from: PeerId,
        msg: NetworkMsg,
        myself: ActorRef<Msg<Ctx>>,
        _state: &mut State<Ctx>,
    ) -> Result<(), ractor::ActorProcessingErr> {
        match msg {
            NetworkMsg::Vote(signed_vote) => {
                let signed_vote = SignedVote::<Ctx>::from_proto(signed_vote).unwrap(); // FIXME
                let validator_address = signed_vote.validator_address();

                info!(%from, %validator_address, "Received vote: {:?}", signed_vote.vote);

                let Some(validator) = self.params.validator_set.get_by_address(validator_address)
                else {
                    warn!(%from, %validator_address, "Received vote from unknown validator");
                    return Ok(());
                };

                if self
                    .ctx
                    .verify_signed_vote(&signed_vote, validator.public_key())
                {
                    myself.cast(Msg::SendDriverInput(driver::Input::Vote(signed_vote.vote)))?;
                } else {
                    warn!(%from, %validator_address, "Received invalid vote: {signed_vote:?}");
                }
            }

            NetworkMsg::Proposal(proposal) => {
                let signed_proposal = SignedProposal::<Ctx>::from_proto(proposal).unwrap();
                let validator_address = signed_proposal.proposal.validator_address();

                info!(%from, %validator_address, "Received proposal: {:?}", signed_proposal.proposal);

                let Some(validator) = self.params.validator_set.get_by_address(validator_address)
                else {
                    warn!(%from, %validator_address, "Received proposal from unknown validator");
                    return Ok(());
                };

                let valid = self
                    .ctx
                    .verify_signed_proposal(&signed_proposal, validator.public_key());

                myself.cast(Msg::SendDriverInput(driver::Input::Proposal(
                    signed_proposal.proposal,
                    Validity::from_valid(valid),
                )))?;
            }

            #[cfg(test)]
            NetworkMsg::Dummy(_) => unreachable!(),
        }

        Ok(())
    }

    pub async fn handle_timeout(
        &self,
        timeout: Timeout,
        myself: ActorRef<Msg<Ctx>>,
        state: &mut State<Ctx>,
    ) -> Result<(), ractor::ActorProcessingErr> {
        let height = state.driver.height();
        let round = state.driver.round();

        if timeout.round != round {
            debug!(
                "Ignoring timeout for round {} at height {}, current round: {round}",
                timeout.round, height
            );

            return Ok(());
        }

        info!("{timeout} elapsed at height {height} and round {round}");

        myself.cast(Msg::SendDriverInput(driver::Input::TimeoutElapsed(timeout)))?;

        if timeout.step == TimeoutStep::Commit {
            myself.cast(Msg::MoveToNextHeight)?;
        }

        Ok(())
    }

    pub async fn send_driver_input(
        &self,
        input: driver::Input<Ctx>,
        myself: ActorRef<Msg<Ctx>>,
        state: &mut State<Ctx>,
    ) -> Result<(), ractor::ActorProcessingErr> {
        match &input {
            driver::Input::NewRound(_, _) => {
                state.timers.cast(TimerMsg::Reset)?;
            }

            driver::Input::ProposeValue(round, _) => state
                .timers
                .cast(TimerMsg::CancelTimeout(Timeout::propose(*round)))?,

            driver::Input::Proposal(proposal, _) => {
                let round = Proposal::<Ctx>::round(proposal);
                state
                    .timers
                    .cast(TimerMsg::CancelTimeout(Timeout::propose(round)))?;
            }

            driver::Input::Vote(_) => (),
            driver::Input::TimeoutElapsed(_) => (),
        }

        let check_threshold = if let driver::Input::Vote(vote) = &input {
            let round = Vote::<Ctx>::round(vote);
            let value = Vote::<Ctx>::value(vote);

            Some((vote.vote_type(), round, value.clone()))
        } else {
            None
        };

        let outputs = state.driver.process(input).unwrap();

        // When we receive a vote, check if we've gotten +2/3 votes for the value we just received a vote for,
        // if so then cancel the corresponding timeout.
        if let Some((vote_type, round, value)) = check_threshold {
            let threshold = match value {
                NilOrVal::Nil => Threshold::Nil,
                NilOrVal::Val(value) => Threshold::Value(value),
            };

            if state
                .driver
                .votes()
                .is_threshold_met(&round, vote_type, threshold.clone())
            {
                let timeout = match vote_type {
                    VoteType::Prevote => Timeout::prevote(round),
                    VoteType::Precommit => Timeout::precommit(round),
                };

                info!("Threshold met for {threshold:?} at round {round}, cancelling {timeout}");
                state.timers.cast(TimerMsg::CancelTimeout(timeout))?;
            }
        }

        for output in outputs {
            match self
                .handle_driver_output(output, myself.clone(), state)
                .await?
            {
                Next::None => (),

                Next::Input(input) => myself.cast(Msg::SendDriverInput(input))?,

                Next::Decided(round, value) => {
                    state
                        .timers
                        .cast(TimerMsg::ScheduleTimeout(Timeout::commit(round)))?;

                    myself.cast(Msg::Decided(state.driver.height(), round, value))?;
                }
            }
        }

        Ok(())
    }

    async fn handle_driver_output(
        &self,
        output: driver::Output<Ctx>,
        myself: ActorRef<Msg<Ctx>>,
        state: &mut State<Ctx>,
    ) -> Result<Next<Ctx>, ActorProcessingErr> {
        match output {
            driver::Output::NewRound(height, round) => {
                info!("New round at height {height}: {round}");

                Ok(Next::Input(driver::Input::NewRound(height, round)))
            }

            driver::Output::Propose(proposal) => {
                info!(
                    "Proposing value {:?} at round {}",
                    proposal.value(),
                    proposal.round()
                );

                let signed_proposal = self.ctx.sign_proposal(proposal);

                // TODO: Refactor to helper method
                let proto = signed_proposal.to_proto().unwrap(); // FIXME
                let msg = NetworkMsg::Proposal(proto);
                let bytes = msg.to_network_bytes().unwrap(); // FIXME
                self.gossip.cast(GossipMsg::Broadcast(bytes))?;

                Ok(Next::Input(driver::Input::Proposal(
                    signed_proposal.proposal,
                    Validity::Valid,
                )))
            }

            driver::Output::Vote(vote) => {
                info!(
                    "Voting for value {:?} at round {}",
                    vote.value(),
                    vote.round()
                );

                let signed_vote = self.ctx.sign_vote(vote);

                // TODO: Refactor to helper method
                let proto = signed_vote.to_proto().unwrap(); // FIXME
                let msg = NetworkMsg::Vote(proto);
                let bytes = msg.to_network_bytes().unwrap(); // FIXME
                self.gossip.cast(GossipMsg::Broadcast(bytes))?;

                Ok(Next::Input(driver::Input::Vote(signed_vote.vote)))
            }

            driver::Output::Decide(round, value) => {
                info!("Decided on value {value:?} at round {round}");

                Ok(Next::Decided(round, value))
            }

            driver::Output::ScheduleTimeout(timeout) => {
                info!("Scheduling {timeout}");
                state.timers.cast(TimerMsg::ScheduleTimeout(timeout))?;

                Ok(Next::None)
            }

            driver::Output::GetValue(height, round, timeout) => {
                info!("Requesting value at height {height} and round {round}");
                self.get_value(myself, height, round, timeout).await?;

                Ok(Next::None)
            }
        }
    }

    pub async fn get_value(
        &self,
        myself: ActorRef<Msg<Ctx>>,
        height: Ctx::Height,
        round: Round,
        _timeout: Timeout,
    ) -> Result<(), ActorProcessingErr> {
        // FIXME: state.timers.timeout_duration(&timeout.step);
        let deadline = Instant::now() + Duration::from_secs(1);

        // FIXME: Use call_and_forward

        let pb = self.params.proposal_builder.clone();

        tokio::task::spawn(async move {
            if let Some(value) = pb.build_proposal(height, deadline).await {
                myself
                    .cast(Msg::SendDriverInput(driver::Input::ProposeValue(
                        round, value,
                    )))
                    .unwrap(); // FIXME
            } else {
                error!("Failed to get value at height {height} and round {round}");
            }
        });

        Ok(())
    }
}

#[ractor::async_trait]
impl<Ctx> Actor for Node<Ctx>
where
    Ctx: Context,
    Ctx::Height: Display,
    Ctx::Vote: Protobuf<Proto = proto::Vote>,
    Ctx::Proposal: Protobuf<Proto = proto::Proposal>,
{
    type Msg = Msg<Ctx>;
    type State = State<Ctx>;
    type Arguments = Args;

    async fn pre_start(
        &self,
        myself: ActorRef<Msg<Ctx>>,
        _args: Args,
    ) -> Result<State<Ctx>, ractor::ActorProcessingErr> {
        let (timers, _) =
            Timers::spawn_linked(self.timers_config, myself.clone(), myself.get_cell()).await?;

        let forward = forward(myself.clone(), Some(myself.get_cell()), Msg::GossipEvent).await?;
        self.gossip.cast(GossipMsg::Subscribe(forward))?;

        let driver = Driver::new(
            self.ctx.clone(),
            self.params.start_height,
            self.params.proposer_selector.clone(),
            self.params.validator_set.clone(),
            self.params.address.clone(),
            self.params.threshold_params,
        );

        Ok(State { driver, timers })
    }

    async fn handle(
        &self,
        myself: ActorRef<Msg<Ctx>>,
        msg: Msg<Ctx>,
        state: &mut State<Ctx>,
    ) -> Result<(), ractor::ActorProcessingErr> {
        match msg {
            Msg::StartHeight(height) => {
                info!("Starting height {height}");

                myself.cast(Msg::SendDriverInput(driver::Input::NewRound(
                    height,
                    Round::new(0),
                )))?;
            }

            Msg::MoveToNextHeight => {
                let height = state.driver.height().increment();
                info!("Moving to next height {height}");

                state.timers.cast(TimerMsg::Reset)?;
                state.driver.move_to_height(height);

                debug_assert_eq!(state.driver.height(), height);
                debug_assert_eq!(state.driver.round(), Round::Nil);
            }

            Msg::Decided(height, round, value) => {
                info!("Decided on value {value:?} at height {height} and round {round}");
            }

            Msg::GossipEvent(event) => {
                self.handle_gossip_event(event.as_ref(), myself, state)
                    .await?;
            }

            Msg::TimeoutElapsed(timeout) => {
                self.handle_timeout(timeout, myself, state).await?;
            }

            Msg::SendDriverInput(input) => {
                self.send_driver_input(input, myself, state).await?;
            }
        }

        Ok(())
    }
}
