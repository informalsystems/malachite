#!/bin/bash

set -e

print_help() {
  echo "Usage: $0 <network_name> <action>"
  echo
  echo "Manage Docker Compose for the given network."
  echo
  echo "Arguments:"
  echo "  network_name   Name of the network (e.g., m2-s3)"
  echo "  action         up | down"
  exit 1
}

if [[ "$1" == "-h" || "$1" == "--help" || -z "$1" || -z "$2" ]]; then
  print_help
fi

network_name="$1"
action="$2"
compose_file="shared/networks/${network_name}/docker-compose.yml"

if [[ ! -f "$compose_file" ]]; then
  echo "Error: Compose file not found at $compose_file"
  exit 1
fi

case "$action" in
  up)
    docker compose -f "$compose_file" up -d || exit 1
    echo "Enter the nodes container with:"
    for container in $(docker compose -f "$compose_file" ps --format '{{.Name}}'); do
      echo -e "\tdocker exec -it $container /bin/bash"
    done
    ;;
  down)
    docker compose -f "$compose_file" down
    ;;
  *)
    echo "Error: Invalid action '$action'. Use 'up' or 'down'."
    print_help
    ;;
esac
