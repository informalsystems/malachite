//! Utility functions for spawning the actor system and connecting it to the application.

use std::path::Path;
use std::time::Duration;

use eyre::Result;
use tracing::Span;

use malachite_engine::consensus::{Consensus, ConsensusCodec, ConsensusParams, ConsensusRef};
use malachite_engine::host::HostRef;
use malachite_engine::network::{Network, NetworkRef};
use malachite_engine::sync::{Params as SyncParams, Sync, SyncCodec, SyncRef};
use malachite_engine::util::events::TxEvent;
use malachite_engine::wal::{Wal, WalCodec, WalRef};
use malachite_network::{Config as NetworkConfig, DiscoveryConfig, GossipSubConfig, Keypair};

use crate::types::config::{Config as NodeConfig, PubSubProtocol, SyncConfig, TransportProtocol};
use crate::types::core::Context;
use crate::types::metrics::{Metrics, SharedRegistry};
use crate::types::sync;
use crate::types::ValuePayload;

pub async fn spawn_network_actor<Ctx, Codec>(
    cfg: &NodeConfig,
    keypair: Keypair,
    registry: &SharedRegistry,
    codec: Codec,
) -> Result<NetworkRef<Ctx>>
where
    Ctx: Context,
    Codec: ConsensusCodec<Ctx>,
    Codec: SyncCodec<Ctx>,
{
    let config = make_gossip_config(cfg);

    Network::spawn(keypair, config, registry.clone(), codec, Span::current())
        .await
        .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
pub async fn spawn_consensus_actor<Ctx>(
    initial_height: Ctx::Height,
    initial_validator_set: Ctx::ValidatorSet,
    address: Ctx::Address,
    ctx: Ctx,
    cfg: NodeConfig,
    network: NetworkRef<Ctx>,
    host: HostRef<Ctx>,
    wal: WalRef<Ctx>,
    sync: Option<SyncRef<Ctx>>,
    metrics: Metrics,
    tx_event: TxEvent<Ctx>,
) -> Result<ConsensusRef<Ctx>>
where
    Ctx: Context,
{
    use crate::types::config;
    let value_payload = match cfg.consensus.value_payload {
        config::ValuePayload::PartsOnly => ValuePayload::PartsOnly,
        config::ValuePayload::ProposalOnly => ValuePayload::ProposalOnly,
        config::ValuePayload::ProposalAndParts => ValuePayload::ProposalAndParts,
    };

    let consensus_params = ConsensusParams {
        initial_height,
        initial_validator_set,
        address,
        threshold_params: Default::default(),
        value_payload,
    };

    Consensus::spawn(
        ctx,
        consensus_params,
        cfg.consensus.timeouts,
        network,
        host,
        wal,
        sync,
        metrics,
        tx_event,
        Span::current(),
    )
    .await
    .map_err(Into::into)
}

pub async fn spawn_wal_actor<Ctx, Codec>(
    ctx: &Ctx,
    codec: Codec,
    home_dir: &Path,
    registry: &SharedRegistry,
) -> Result<WalRef<Ctx>>
where
    Ctx: Context,
    Codec: WalCodec<Ctx>,
{
    let wal_dir = home_dir.join("wal");
    std::fs::create_dir_all(&wal_dir).unwrap();

    let wal_file = wal_dir.join("consensus.wal");

    Wal::spawn(ctx, codec, wal_file, registry.clone(), Span::current())
        .await
        .map_err(Into::into)
}

pub async fn spawn_sync_actor<Ctx>(
    ctx: Ctx,
    network: NetworkRef<Ctx>,
    host: HostRef<Ctx>,
    config: &SyncConfig,
    registry: &SharedRegistry,
) -> Result<Option<SyncRef<Ctx>>>
where
    Ctx: Context,
{
    if !config.enabled {
        return Ok(None);
    }

    let params = SyncParams {
        status_update_interval: config.status_update_interval,
        request_timeout: config.request_timeout,
    };

    let metrics = sync::Metrics::register(registry);

    let actor_ref = Sync::spawn(ctx, network, host, params, metrics, Span::current()).await?;

    Ok(Some(actor_ref))
}

fn make_gossip_config(cfg: &NodeConfig) -> NetworkConfig {
    NetworkConfig {
        listen_addr: cfg.consensus.p2p.listen_addr.clone(),
        persistent_peers: cfg.consensus.p2p.persistent_peers.clone(),
        discovery: DiscoveryConfig {
            enabled: cfg.consensus.p2p.discovery.enabled,
            ..Default::default()
        },
        idle_connection_timeout: Duration::from_secs(15 * 60),
        transport: match cfg.consensus.p2p.transport {
            TransportProtocol::Tcp => malachite_network::TransportProtocol::Tcp,
            TransportProtocol::Quic => malachite_network::TransportProtocol::Quic,
        },
        pubsub_protocol: match cfg.consensus.p2p.protocol {
            PubSubProtocol::GossipSub(_) => malachite_network::PubSubProtocol::GossipSub,
            PubSubProtocol::Broadcast => malachite_network::PubSubProtocol::Broadcast,
        },
        gossipsub: match cfg.consensus.p2p.protocol {
            PubSubProtocol::GossipSub(config) => GossipSubConfig {
                mesh_n: config.mesh_n(),
                mesh_n_high: config.mesh_n_high(),
                mesh_n_low: config.mesh_n_low(),
                mesh_outbound_min: config.mesh_outbound_min(),
            },
            PubSubProtocol::Broadcast => GossipSubConfig::default(),
        },
        rpc_max_size: cfg.consensus.p2p.rpc_max_size.as_u64() as usize,
        pubsub_max_size: cfg.consensus.p2p.pubsub_max_size.as_u64() as usize,
    }
}
