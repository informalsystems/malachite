#!/usr/bin/python3
import sys
import csv
import subprocess
import os
import socket

def read_matrix(csv_file):
    with open(csv_file) as f:
        reader = csv.reader(f)
        header = next(reader)[1:]
        matrix = {}
        for row in reader:
            row_zone = row[0]
            matrix[row_zone] = {}
            for i, col_zone in enumerate(header):
                matrix[row_zone][col_zone] = int(row[i+1])
    return header, matrix

def resolve_ips(hostnames):
    return {host: socket.gethostbyname(host) for host in hostnames}

def execute_command(cmd):
    subprocess.run(cmd, shell=True, check=True)

def build_tc_commands(header, matrix, local_dns, ip_map):
    commands = []
    commands.append("tc qdisc del dev eth0 root 2> /dev/null || true")
    commands.append("tc qdisc add dev eth0 root handle 1: htb default 10")
    commands.append("tc class add dev eth0 parent 1: classid 1:1 htb rate 1gbit")
    commands.append("tc class add dev eth0 parent 1:1 classid 1:10 htb rate 1gbit")
    commands.append("tc qdisc add dev eth0 parent 1:10 handle 10: sfq perturb 10")

    handle = 11
    for zone in header:
        if zone == local_dns:
            continue
        latency = matrix[local_dns][zone]
        if latency > 0:
            delta = latency // 20 or 1
            commands.append(f"tc class add dev eth0 parent 1:1 classid 1:{handle} htb rate 1gbit")
            commands.append(f"tc qdisc add dev eth0 parent 1:{handle} handle {handle}: netem delay {latency}ms {delta}ms distribution normal")
            commands.append(f"tc filter add dev eth0 protocol ip parent 1: prio 1 u32 match ip dst {ip_map[zone]}/32 flowid 1:{handle}")
            handle += 1
    return commands

def main():
    csv_file = sys.argv[1]
    local_dns = os.environ["LOCAL_DNS"]
    header, matrix = read_matrix(csv_file)
    ip_map = resolve_ips(header)
    commands = build_tc_commands(header, matrix, local_dns, ip_map)
    for cmd in commands:
        execute_command(cmd)

if __name__ == "__main__":
    main()
