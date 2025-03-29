#!/bin/bash

for container in $(docker compose ps -q);
do
  docker exec "$container" /scripts/apply-tc-rules.py /scripts/latencies.csv
done
