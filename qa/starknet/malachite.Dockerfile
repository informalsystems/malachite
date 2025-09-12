FROM rust:1-slim

RUN apt-get update && apt-get install -y bash bash-completion protobuf-compiler python3 iproute2 make procps librust-tikv-jemalloc-sys-dev && \
    echo 'set editing-mode emacs' >> /etc/inputrc && \
    echo '[[ $PS1 && -f /etc/bash_completion ]] && . /etc/bash_completion' >> /etc/bash.bashrc

SHELL [ "/bin/bash", "-c" ]
