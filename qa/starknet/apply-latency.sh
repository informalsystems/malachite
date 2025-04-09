#!/bin/bash

set -e

print_help() {
  echo "Usage: $0 <network_name>"
  echo
  echo "Apply TC rules to all containers in the given Docker Compose network."
  echo
  echo "Arguments:"
  echo "  network_name   Name of the network (e.g., m2-s3)"
  exit 1
}

if [[ "$1" == "-h" || "$1" == "--help" || -z "$1" ]]; then
  print_help
fi

network_name="$1"
compose_file="shared/networks/${network_name}/docker-compose.yml"

if [[ ! -f "$compose_file" ]]; then
  echo "Error: Compose file not found at $compose_file"
  exit 1
fi

for container in $(docker compose -f "$compose_file" ps -q); do
  docker exec "$container" /shared/scripts/apply-tc-rules.py /shared/networks/$network_name/latencies.csv
done
