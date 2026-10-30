#!/bin/sh
# OpenSSH's server, and its client (ssh-add, to see a forwarded agent), from
# whichever package manager the image has.
set -eu
if command -v apk >/dev/null; then
    apk add --no-cache openssh
elif command -v apt-get >/dev/null; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends openssh-server openssh-client
    rm -rf /var/lib/apt/lists/*
elif command -v dnf >/dev/null; then
    dnf install -y openssh-server openssh-clients
    dnf clean all
else
    echo "no package manager this knows" >&2
    exit 1
fi
