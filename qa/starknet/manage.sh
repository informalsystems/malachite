#!/bin/bash

set -e

print_help() {
  echo "Usage: $0 <network_name> <action> [duration]"
  echo
  echo "Manage Docker Compose for the given network."
  echo
  echo "Arguments:"
  echo "  network_name   Name of the network (e.g., m2-s3)"
  echo "  action         up | down | build | start | stop | reset"
  echo "  duration       Number of seconds (required only if action=start)"
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
    for container in $(docker compose -f "$compose_file" ps --format '{{.Name}}'); do
      echo "docker exec -it $container /bin/bash"
    done
    ;;
  down)
    docker compose -f "$compose_file" down
    ;;
  build)
    containers=( $(docker compose -f "$compose_file" ps --format '{{.Name}}' \
      | grep -E '(malachite-node-1|sequencer-node-1)') )
    for container in "${containers[@]}"; do
      docker exec -it "$container" /bin/bash -lic "build"
    done
    ;;
  start)
    duration="$3"
    if [[ -z "$duration" ]]; then
      echo "Error: 'start' requires a duration in seconds"
      exit 1
    fi
    echo "Starting the nodes..."
    containers=( $(docker compose -f "$compose_file" ps --format '{{.Name}}' ) )
    for container in "${containers[@]}"; do
      if [[ "$container" =~ ^malachite-node- ]]; then
        docker exec -it "$container" /bin/bash -lic "sleep 10 && start > /shared/networks/${network_name}/logs/${container}.log 2>&1 &"
      else
        docker exec -it "$container" /bin/bash -lic "start > /shared/networks/${network_name}/logs/${container}.log 2>&1 &"
      fi
    done
    sleep 10 # The sequencer takes about 5 seconds to start
    echo "Running for $duration seconds..."
    sleep "$duration"
    for container in "${containers[@]}"; do
      docker exec -it "$container" /bin/bash -lic "pkill -f shared"
    done
    ;;
  stop)
    docker compose -f "$compose_file" stop
    ;;
  reset)
    containers=( $(docker compose -f "$compose_file" ps --format '{{.Name}}' ) )
    for container in "${containers[@]}"; do
      docker exec -it "$container" /bin/bash -lic "reset"
    done
    ;;
  *)
    echo "Error: Invalid action '$action'. Use 'up', 'down', 'build', or 'start'."
    print_help
    ;;
esac
