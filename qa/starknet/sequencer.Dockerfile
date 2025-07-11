FROM ghcr.io/informalsystems/sequencer:latest

RUN sudo apt-get update && sudo apt-get install -y bash bash-completion python3 iproute2 procps

SHELL [ "/bin/bash", "-c" ]
