#!/bin/sh
# OpenSSH's server, its client (ssh-add, to see a forwarded agent), and what
# X11 forwarding needs there (xauth, and xdpyinfo as a program to forward),
# from whichever package manager the image has.
set -eu
if command -v apk >/dev/null; then
    apk add --no-cache openssh xauth xdpyinfo
elif command -v apt-get >/dev/null; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends openssh-server openssh-client xauth x11-utils
    rm -rf /var/lib/apt/lists/*
elif command -v dnf >/dev/null; then
    dnf install -y openssh-server openssh-clients xorg-x11-xauth xdpyinfo
    dnf clean all
else
    echo "no package manager this knows" >&2
    exit 1
fi
