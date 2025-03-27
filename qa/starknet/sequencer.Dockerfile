FROM rust:latest

RUN apt-get update && apt-get install -y bash bash-completion && \
    echo 'set editing-mode emacs' >> /etc/inputrc && \
    echo '[[ $PS1 && -f /etc/bash_completion ]] && . /etc/bash_completion' >> /etc/bash.bashrc

COPY sequencer/scripts/install_build_tools.sh .

COPY sequencer/scripts/dependencies.sh .

RUN ./install_build_tools.sh

SHELL [ "/bin/bash", "-c" ]
