#!/usr/bin/env python3

import os
import argparse
import base64
import json
import csv
import libp2p.peer.id as PeerID
import libp2p.crypto.ed25519 as Ed25519
from jinja2 import Environment, FileSystemLoader
from eth_hash.auto import keccak
from nacl.signing import SigningKey

ADDRESS_LENGTH = 31


def generate_key():
    signing_key = SigningKey.generate()
    verify_key = signing_key.verify_key

    private_key_bytes = signing_key.encode()
    public_key_bytes = verify_key.encode()
    private_key_b64 = base64.b64encode(private_key_bytes).decode()
    public_key_b64 = base64.b64encode(public_key_bytes).decode()
    private_key_hex = "0x" + private_key_bytes.hex()
    address = "0x" + keccak(public_key_bytes)[:ADDRESS_LENGTH].hex()

    key_data = {
        "private_key": {"type": "tendermint/PrivKeyEd25519", "value": private_key_b64},
        "public_key": {"type": "tendermint/PubKeyEd25519", "value": public_key_b64},
        "address": address,
    }

    peer_id = PeerID.ID.from_pubkey(
        Ed25519.Ed25519PublicKey.from_bytes(public_key_bytes)
    )

    return key_data, private_key_hex, peer_id


def save_malachite_config(dir, rendered_config, genesis, key_data):
    os.makedirs(dir, exist_ok=True)
    with open(f"{dir}/config.toml", "w") as c:
        c.write(rendered_config)
    with open(f"{dir}/genesis.json", "w") as g:
        g.write(json.dumps(genesis, indent=4))
    with open(f"{dir}/priv_validator_key.json", "w") as p:
        p.write(json.dumps(key_data, indent=4))


def save_sequencer_config(dir, rendered_config):
    os.makedirs(dir, exist_ok=True)
    with open(f"{dir}/config.json", "w") as c:
        c.write(json.dumps(rendered_config, indent=4))


def save_cli(dir, cmd, rendered_cli):
    os.makedirs(dir, exist_ok=True)
    with open(f"{dir}/{cmd}.sh", "w") as c:
        c.write(rendered_cli)
    os.chmod(f"{dir}/{cmd}.sh", 0o755)


def save_bashrc(dir, rendered_bashrc):
    os.makedirs(dir, exist_ok=True)
    with open(f"{dir}/.bashrc", "w") as c:
        c.write(rendered_bashrc)
    os.chmod(f"{dir}/.bashrc", 0o755)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--name", type=str, required=False, help="Name of the network")
    parser.add_argument(
        "--malachite_path",
        type=str,
        required=True,
        help="Absolute path to malachite repository",
    )
    parser.add_argument(
        "--sequencer_path",
        type=str,
        required=True,
        help="Absolute path to sequencer repository",
    )
    parser.add_argument(
        "--malachite_nodes", type=int, required=True, help="Number of malachite nodes"
    )
    parser.add_argument(
        "--sequencer_nodes", type=int, required=True, help="Number of sequencer nodes"
    )
    parser.add_argument(
        "--proposal_timeout",
        type=int,
        required=False,
        default=500,
        help="Proposal timeout (in ms)",
    )
    parser.add_argument(
        "--prevote_timeout",
        type=int,
        required=False,
        default=500,
        help="Prevote timeout (in ms)",
    )
    parser.add_argument(
        "--precommit_timeout",
        type=int,
        required=False,
        default=500,
        help="Precommit timeout (in ms)",
    )
    parser.add_argument(
        "--latency",
        type=int,
        required=False,
        default=0,
        help="Latency (in ms) between nodes",
    )
    args = parser.parse_args()

    if not args.name:
        args.name = f"m{args.malachite_nodes}-s{args.sequencer_nodes}"

    # Create base directory for the network
    base_dir = f"./shared/networks/{args.name}"
    os.makedirs(base_dir, exist_ok=True)

    env = Environment(loader=FileSystemLoader("."))

    compose_template = env.get_template("templates/docker-compose.j2")
    rendered_compose = compose_template.render(
        network_name=args.name,
        malachite_path=args.malachite_path,
        sequencer_path=args.sequencer_path,
        malachite_count=args.malachite_nodes,
        sequencer_count=args.sequencer_nodes,
    )
    with open(f"{base_dir}/docker-compose.yml", "w") as f:
        f.write(rendered_compose)

    malachite_data = []
    sequencer_data = []
    genesis = {"validator_set": {"validators": []}}
    addresses = []

    # Generate keys and configs for malachite nodes
    malachite_config_template = env.get_template("templates/malachite-config.j2")
    for i in range(1, args.malachite_nodes + 1):
        peers = []
        for j in range(1, args.malachite_nodes + 1):
            if j != i:
                peers.append(f"malachite-node-{j}")
        for k in range(1, args.sequencer_nodes + 1):
            peers.append(f"sequencer-node-{k}")

        rendered_config = malachite_config_template.render(
            persistent_peers=peers,
            proposal_timeout=f"{args.proposal_timeout}ms",
            prevote_timeout=f"{args.prevote_timeout}ms",
            precommit_timeout=f"{args.precommit_timeout}ms",
        )
        key_data, _, _ = generate_key()

        malachite_data.append(
            {
                "rendered_config": rendered_config,
                "key_data": key_data,
            }
        )

        genesis["validator_set"]["validators"].append(
            {
                "address": key_data["address"],
                "public_key": key_data["public_key"],
                "voting_power": 1,
            }
        )

        addresses.append(key_data["address"])

    # Generate keys and config for sequencer nodes
    # TODO: for now, there is no config file for sequencer nodes
    for i in range(1, args.sequencer_nodes + 1):
        key_data, private_key_hex, peer_id = generate_key()

        sequencer_data.append(
            {
                "key_data": key_data,
                "private_key_hex": private_key_hex,
                "peer_id": peer_id,
            }
        )

        genesis["validator_set"]["validators"].append(
            {
                "address": key_data["address"],
                "public_key": key_data["public_key"],
                "voting_power": 1,
            }
        )

        addresses.append(key_data["address"])

    # Save malachite configs
    for i, data in enumerate(malachite_data):
        save_malachite_config(
            f"{base_dir}/malachite-node-{i + 1}/config",
            data["rendered_config"],
            genesis,
            data["key_data"],
        )

    # Save sequencer configs
    # TODO: for now, there is no config file for sequencer nodes

    bashrc_template = env.get_template("templates/bashrc.j2")

    # Generate and save malachite cli
    malachite_cli_start_template = env.get_template("templates/malachite-cli-start.j2")
    malachite_cli_reset_template = env.get_template("templates/malachite-cli-reset.j2")
    for i in range(1, args.malachite_nodes + 1):
        rendered_cli = malachite_cli_start_template.render(network_name=args.name, id=i)
        save_cli(f"{base_dir}/malachite-node-{i}/", "start", rendered_cli)
        rendered_cli = malachite_cli_reset_template.render(network_name=args.name, id=i)
        save_cli(f"{base_dir}/malachite-node-{i}/", "reset", rendered_cli)

        rendered_bashrc = bashrc_template.render(
            network_name=args.name,
            node_type="malachite",
            id=i,
        )
        save_bashrc(f"{base_dir}/malachite-node-{i}/", rendered_bashrc)

    # Generate and save sequencer cli
    sequencer_cli_start_template = env.get_template("templates/sequencer-cli-start.j2")
    sequencer_cli_reset_template = env.get_template("templates/sequencer-cli-reset.j2")

    if args.proposal_timeout <= 1000:
        print(
            "Warning: Proposal timeout should be > 1s for the sequencer. Setting it to 1001ms."
        )
        args.proposal_timeout = 1001

    for i in range(1, args.sequencer_nodes + 1):
        rendered_cli = sequencer_cli_start_template.render(
            network_name=args.name,
            id=i,
            consensus_secret_key=sequencer_data[i - 1]["private_key_hex"],
            validator_id=sequencer_data[i - 1]["key_data"]["address"],
            num_validators=args.malachite_nodes + args.sequencer_nodes,
            validator_ids=",".join(addresses),
            bootstrap_peer=(
                f'/dns/sequencer-node-{((i - 2) % args.sequencer_nodes) + 1}/tcp/27000/p2p/{sequencer_data[(i - 2) % args.sequencer_nodes]["peer_id"]}'
                if args.sequencer_nodes >= 2
                else None
            ),
            proposal_timeout=args.proposal_timeout / 1000,
            prevote_timeout=args.prevote_timeout / 1000,
            precommit_timeout=args.precommit_timeout / 1000,
        )
        save_cli(f"{base_dir}/sequencer-node-{i}/", "start", rendered_cli)
        rendered_cli = sequencer_cli_reset_template.render(network_name=args.name, id=i)
        save_cli(f"{base_dir}/sequencer-node-{i}/", "reset", rendered_cli)

        rendered_bashrc = bashrc_template.render(
            network_name=args.name,
            node_type="sequencer",
            id=i,
        )
        save_bashrc(f"{base_dir}/sequencer-node-{i}/", rendered_bashrc)

    # Generate and save latency config
    data = [
        ["From/To"]
        + [f"malachite-node-{i}" for i in range(1, args.malachite_nodes + 1)]
        + [f"sequencer-node-{i}" for i in range(1, args.sequencer_nodes + 1)]
    ]
    for i in range(1, args.malachite_nodes + 1):
        row = [f"malachite-node-{i}"] + [args.latency] * (
            args.malachite_nodes + args.sequencer_nodes
        )
        data.append(row)
    for i in range(1, args.sequencer_nodes + 1):
        row = [f"sequencer-node-{i}"] + [args.latency] * (
            args.malachite_nodes + args.sequencer_nodes
        )
        data.append(row)
    with open(f"{base_dir}/latencies.csv", "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerows(data)


if __name__ == "__main__":
    main()
