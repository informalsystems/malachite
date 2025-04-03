# Starknet Interoperability Setup

## Setup

Clone the Starknet sequencer fork:

```bash
git clone https://github.com/bastienfaivre/sequencer.git
cd sequencer
git checkout for_informalsystems/mock_batcher_to_return_empty_proposals
```

> [!CAUTION]
> The setup assumes that both malachite and sequencer repositories are cloned in the same directory.

```bash
cd malachite/qa/starknet
docker compose up -d
```

Then, you need to manually enter into the containers to start the nodes.

```bash
docker exec -it <CONTAINER NAME> /bin/bash
```

The names can be found in the `docker-compose.yml` file.

### Latency

Latencies between nodes is defined in `shared/scripts/latencies.csv`. To apply it, run:

```bash
./shared/scripts/apply-tc-rules-all.sh
```

## Build

You might want to build the nodes to avoid re-building when running with `cargo run`. For that, simply run:

```bash
cargo build --release
```

>[!NOTE]
> The flag `--release` is not necessary. But if you remove it, you need to change the path of all executables in the commands below to `/shared/build/<malachite|sequencer>/debug/<executable>`.

Once per node type (malachite and sequencer) in one of the respective containers.

The builds will persist in the `/shared/build` directory and you will not need to rebuild them when restarting the containers.

## Run

### Build version

#### Malachite #1

```bash
rm -rf /shared/config/malachite-node-1/db /shared/config/malachite-node-1/wal
/shared/build/malachite/release/informalsystems-malachitebft-starknet-app start --home /shared/config/malachite-node-1
```

#### Malachite #2

```bash
rm -rf /shared/config/malachite-node-2/db /shared/config/malachite-node-2/wal
/shared/build/malachite/release/informalsystems-malachitebft-starknet-app start --home /shared/config/malachite-node-2
```

#### Sequencer #1

```bash
rm -rf /shared/logs/sequencer-node-1/*
RUST_LOG=starknet_consensus=debug,starknet=info,papyrus_network=debug,papyrus=info /shared/build/sequencer/release/starknet_sequencer_node --chain_id MY_CUSTOM_CHAIN_ID --eth_fee_token_address 0x1001 --strk_fee_token_address 0x1002 --recorder_url http://invalid_address.com --base_layer_config.node_url http://invalid_address.com --batcher_config.storage.db_config.path_prefix /shared/logs/sequencer-node-1/batcher_data --class_manager_config.class_storage_config.class_hash_storage_config.path_prefix /shared/logs/sequencer-node-1/class_manager_data --state_sync_config.storage_config.db_config.path_prefix /shared/logs/sequencer-node-1/sync_data --consensus_manager_config.network_config.tcp_port 27000 --mempool_p2p_config.network_config.tcp_port 11000 --state_sync_config.network_config.tcp_port 12000 --http_server_config.port 13000 --monitoring_endpoint_config.port 14000 --consensus_manager_config.network_config.secret_key 0x1111111111111111111111111111111111111111111111111111111111111111 --state_sync_config.network_config.secret_key 0x2222222222222222222222222222222222222222222222222222222222222222 --validator_id 0x64 --consensus_manager_config.context_config.num_validators 4 --consensus_manager_config.context_config.validator_ids 0x64,0x65,0x4b58ef72fbd19638006e6eb584d31b16e816a48e0bff16532804e378039588a,0xf2e1d11cae728b3c5cde867cc0fd5d81e3958d52652ec546526e8f8002d0f8
```

#### Sequencer #2

```bash
rm -rf /shared/logs/sequencer-node-2/*
RUST_LOG=starknet_consensus=debug,starknet=info,papyrus_network=debug,papyrus=info /shared/build/sequencer/release/starknet_sequencer_node --chain_id MY_CUSTOM_CHAIN_ID --eth_fee_token_address 0x1001 --strk_fee_token_address 0x1002 --recorder_url http://invalid_address.com --base_layer_config.node_url http://invalid_address.com --batcher_config.storage.db_config.path_prefix /shared/logs/sequencer-node-2/batcher_data --class_manager_config.class_storage_config.class_hash_storage_config.path_prefix /shared/logs/sequencer-node-2/class_manager_data --state_sync_config.storage_config.db_config.path_prefix /shared/logs/sequencer-node-2/sync_data --consensus_manager_config.network_config.tcp_port 27000 --mempool_p2p_config.network_config.tcp_port 11000 --state_sync_config.network_config.tcp_port 12000 --http_server_config.port 13000 --monitoring_endpoint_config.port 14000 --consensus_manager_config.network_config.secret_key 0x3333333333333333333333333333333333333333333333333333333333333333 --state_sync_config.network_config.secret_key 0x4444444444444444444444444444444444444444444444444444444444444444 --validator_id 0x65 --consensus_manager_config.context_config.num_validators 4 --consensus_manager_config.context_config.validator_ids 0x64,0x65,0x4b58ef72fbd19638006e6eb584d31b16e816a48e0bff16532804e378039588a,0xf2e1d11cae728b3c5cde867cc0fd5d81e3958d52652ec546526e8f8002d0f8 --consensus_manager_config.network_config.bootstrap_peer_multiaddr /dns/sequencer-node-1/tcp/27000/p2p/12D3KooWPqT2nMDSiXUSx5D7fasaxhxKigVhcqfkKqrLghCq9jxz --consensus_manager_config.network_config.bootstrap_peer_multiaddr.#is_none false
```

### Cargo run version

#### Malachite #1

```bash
rm -rf /shared/config/malachite-node-1/db /shared/config/malachite-node-1/wal
cargo run --bin informalsystems-malachitebft-starknet-app -- start --home /shared/config/malachite-node-1
```

#### Malachite #2

```bash
rm -rf /shared/config/malachite-node-2/db /shared/config/malachite-node-2/wal
cargo run --bin informalsystems-malachitebft-starknet-app -- start --home /shared/config/malachite-node-2
```

#### Sequencer #1

```bash
rm -rf /shared/logs/sequencer-node-1/*
RUST_LOG=starknet_consensus=debug,starknet=info,papyrus_network=debug,papyrus=info cargo run --bin starknet_sequencer_node -- --chain_id MY_CUSTOM_CHAIN_ID --eth_fee_token_address 0x1001 --strk_fee_token_address 0x1002 --recorder_url http://invalid_address.com --base_layer_config.node_url http://invalid_address.com --batcher_config.storage.db_config.path_prefix /shared/logs/sequencer-node-1/batcher_data --class_manager_config.class_storage_config.class_hash_storage_config.path_prefix /shared/logs/sequencer-node-1/class_manager_data --state_sync_config.storage_config.db_config.path_prefix /shared/logs/sequencer-node-1/sync_data --consensus_manager_config.network_config.tcp_port 27000 --mempool_p2p_config.network_config.tcp_port 11000 --state_sync_config.network_config.tcp_port 12000 --http_server_config.port 13000 --monitoring_endpoint_config.port 14000 --consensus_manager_config.network_config.secret_key 0x1111111111111111111111111111111111111111111111111111111111111111 --state_sync_config.network_config.secret_key 0x2222222222222222222222222222222222222222222222222222222222222222 --validator_id 0x64 --consensus_manager_config.context_config.num_validators 4 --consensus_manager_config.context_config.validator_ids 0x64,0x65,0x4b58ef72fbd19638006e6eb584d31b16e816a48e0bff16532804e378039588a,0xf2e1d11cae728b3c5cde867cc0fd5d81e3958d52652ec546526e8f8002d0f8
```

#### Sequencer #2

```bash
rm -rf /shared/logs/sequencer-node-2/*
RUST_LOG=starknet_consensus=debug,starknet=info,papyrus_network=debug,papyrus=info cargo run --bin starknet_sequencer_node -- --chain_id MY_CUSTOM_CHAIN_ID --eth_fee_token_address 0x1001 --strk_fee_token_address 0x1002 --recorder_url http://invalid_address.com --base_layer_config.node_url http://invalid_address.com --batcher_config.storage.db_config.path_prefix /shared/logs/sequencer-node-2/batcher_data --class_manager_config.class_storage_config.class_hash_storage_config.path_prefix /shared/logs/sequencer-node-2/class_manager_data --state_sync_config.storage_config.db_config.path_prefix /shared/logs/sequencer-node-2/sync_data --consensus_manager_config.network_config.tcp_port 27000 --mempool_p2p_config.network_config.tcp_port 11000 --state_sync_config.network_config.tcp_port 12000 --http_server_config.port 13000 --monitoring_endpoint_config.port 14000 --consensus_manager_config.network_config.secret_key 0x3333333333333333333333333333333333333333333333333333333333333333 --state_sync_config.network_config.secret_key 0x4444444444444444444444444444444444444444444444444444444444444444 --validator_id 0x67 --consensus_manager_config.context_config.num_validators 4 --consensus_manager_config.context_config.validator_ids 0x64,0x65,0x4b58ef72fbd19638006e6eb584d31b16e816a48e0bff16532804e378039588a,0xf2e1d11cae728b3c5cde867cc0fd5d81e3958d52652ec546526e8f8002d0f8 --consensus_manager_config.network_config.bootstrap_peer_multiaddr /dns/sequencer-node-1/tcp/27000/p2p/12D3KooWPqT2nMDSiXUSx5D7fasaxhxKigVhcqfkKqrLghCq9jxz --consensus_manager_config.network_config.bootstrap_peer_multiaddr.#is_none false
```
