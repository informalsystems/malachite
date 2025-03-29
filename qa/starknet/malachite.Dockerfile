FROM rust:latest

RUN apt-get update && apt-get install -y bash bash-completion protobuf-compiler python3 iproute2 && \
    echo 'set editing-mode emacs' >> /etc/inputrc && \
    echo '[[ $PS1 && -f /etc/bash_completion ]] && . /etc/bash_completion' >> /etc/bash.bashrc

SHELL [ "/bin/bash", "-c" ]
