use tokio::sync::mpsc;

use malachite_actors::util::codec::NetworkCodec;
use malachite_actors::util::streaming::StreamMessage;
use malachite_common::Context;
use malachite_config::Config as NodeConfig;
use malachite_consensus::SignedConsensusMsg;
use malachite_gossip_consensus::Keypair;
use malachite_metrics::{Metrics, SharedRegistry};
use malachite_node::Node;

use crate::channel::AppMsg;
use crate::spawn::{
    spawn_block_sync_actor, spawn_consensus_actor, spawn_gossip_consensus_actor, spawn_host_actor,
};

// Todo: Remove clippy exception when the function signature is finalized
#[allow(clippy::too_many_arguments)]
pub async fn run<N, Ctx, Codec>(
    cfg: NodeConfig,
    start_height: Option<Ctx::Height>,
    ctx: Ctx,
    _node: N, // we will need it to get private/public key, address and eventually KeyPair
    codec: Codec,
    keypair: Keypair,      // Todo: see note in code
    address: Ctx::Address, // Todo: remove it when Node was properly implemented
    initial_validator_set: Ctx::ValidatorSet,
) -> Result<mpsc::Receiver<AppMsg<Ctx>>, String>
where
    N: Node<Context = Ctx>,
    Ctx: Context,
    Codec: NetworkCodec<Ctx::ProposalPart>,
    Codec: NetworkCodec<SignedConsensusMsg<Ctx>>,
    Codec: NetworkCodec<StreamMessage<Ctx::ProposalPart>>,
    Codec: NetworkCodec<malachite_blocksync::Status<Ctx>>,
    Codec: NetworkCodec<malachite_blocksync::Request<Ctx>>,
    Codec: NetworkCodec<malachite_blocksync::Response<Ctx>>,
{
    let start_height = start_height.unwrap_or_default();

    let registry = SharedRegistry::global().with_moniker(cfg.moniker.as_str());
    let metrics = Metrics::register(&registry);

    // The key types are not generic enough to create a gossip_consensus::KeyPair, but the current
    // libp2p implementation requires a KeyPair in SwarmBuilder::with_existing_identity.
    // We either decide on a specific keytype (ed25519 or ecdsa) or keep asking the user for the
    // KeyPair.
    // let private_key = node.load_private_key(node.load_private_key_file(&home_dir).unwrap());
    // let public_key = node.generate_public_key(private_key);
    // let address: Ctx::Address = node.get_address(public_key);
    // let pk_bytes = private_key.inner().to_bytes_be();
    // let secret_key = ecdsa::SecretKey::try_from_bytes(pk_bytes).unwrap();
    // let ecdsa_keypair = ecdsa::Keypair::from(secret_key);
    // Keypair::from(ecdsa_keypair)

    // Spawn consensus gossip
    let gossip_consensus = spawn_gossip_consensus_actor(&cfg, keypair, &registry, codec).await;

    // Spawn the host actor
    let (connector, rx) = spawn_host_actor(metrics.clone()).await;

    let block_sync = spawn_block_sync_actor(
        ctx.clone(),
        gossip_consensus.clone(),
        connector.clone(),
        &cfg.blocksync,
        start_height,
        &registry,
    )
    .await;

    // Spawn consensus
    let _consensus = spawn_consensus_actor(
        start_height,
        initial_validator_set,
        address,
        ctx.clone(),
        cfg,
        gossip_consensus.clone(),
        connector.clone(),
        block_sync.clone(),
        metrics,
        None, // tx_decision
    )
    .await;

    Ok(rx)
}