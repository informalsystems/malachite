# Starknet Interopeability Setup

## Setup

> [!CAUTION]
> The setup assumes that both malachite and sequencer repositories are cloned in the same directory.

```bash
docker compose up -d
```

Then, you need to manually enter into the containers to start the nodes.

```bash
docker exec -it <CONTAINER NAME> /bin/bash
```

The names can be found in the `docker-compose.yml` file.

## Run

### Malachite

```bash
rm -rf /config/malachite-node-<NODE>/db /config/malachite-node-<NODE>/wal
cargo run --bin informalsystems-malachitebft-starknet-app -- start --home /config/malachite-node-<NODE>
```

### Sequencer

```bash
rm -rf /logs/sequencer-node-1/*
RUST_LOG=starknet_consensus=debug,starknet=info,papyrus_network=debug,papyrus=info cargo run --bin starknet_sequencer_node -- --chain_id MY_CUSTOM_CHAIN_ID --eth_fee_token_address 0x1001 --strk_fee_token_address 0x1002 --recorder_url http://invalid_address.com --base_layer_config.node_url http://invalid_address.com --batcher_config.storage.db_config.path_prefix /logs/sequencer-node-1/batcher_data --class_manager_config.class_storage_config.class_hash_storage_config.path_prefix /logs/sequencer-node-1/class_manager_data --state_sync_config.storage_config.db_config.path_prefix /logs/sequencer-node-1/sync_data --consensus_manager_config.network_config.tcp_port 27000 --mempool_p2p_config.network_config.tcp_port 11000 --state_sync_config.network_config.tcp_port 12000 --http_server_config.port 13000 --monitoring_endpoint_config.port 14000 --consensus_manager_config.network_config.secret_key 0x1111111111111111111111111111111111111111111111111111111111111111 --state_sync_config.network_config.secret_key 0x2222222222222222222222222222222222222222222222222222222222222222 --validator_id 0x64 --consensus_manager_config.context_config.num_validators 2
```
