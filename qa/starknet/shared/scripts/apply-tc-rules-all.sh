#!/bin/bash

for container in $(docker compose ps -q);
do
  docker exec "$container" /shared/scripts/apply-tc-rules.py /shared/scripts/latencies.csv
done
