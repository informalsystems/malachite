FROM rust:1-slim

RUN apt-get update && apt-get install -y bash bash-completion python3 iproute2 procps && \
    echo 'set editing-mode emacs' >> /etc/inputrc && \
    echo '[[ $PS1 && -f /etc/bash_completion ]] && . /etc/bash_completion' >> /etc/bash.bashrc

COPY scripts/install_build_tools.sh .
COPY scripts/dependencies.sh .

RUN ./install_build_tools.sh

SHELL [ "/bin/bash", "-c" ]
