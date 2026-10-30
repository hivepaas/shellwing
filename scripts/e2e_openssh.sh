#!/usr/bin/env bash
# End-to-end tests against real OpenSSH servers, in Docker
# (crates/ssh/tests/openssh.rs). A bastion is the one this machine reaches.
# Behind it, on a network only the bastion is on, are servers of several
# OpenSSH versions, as the distributions people run ship them. Keys, CAs and
# the password are made for each run, in a folder removed afterwards.
#
#   scripts/e2e_openssh.sh                        build, start, test, remove
#   scripts/e2e_openssh.sh -- <cargo test args>   e.g. -- sftp
#   SHELLWING_E2E_KEEP=1 scripts/e2e_openssh.sh   leave the servers running
set -euo pipefail

cd "$(dirname "$0")/.."
[[ "${1:-}" == "--" ]] && shift

# The servers behind the bastion: label=image. Each label is also the
# server's host name, as the bastion resolves it.
targets=(
	alpine=alpine:3.22       # OpenSSH 10
	debian12=debian:12       # 9.2
	ubuntu2004=ubuntu:20.04  # 8.2
	rocky8=rockylinux:8      # 8.0
)
bastion_image=alpine:3.22

run="sw-e2e-$$"
work=$(mktemp -d)
cleanup() {
	if [[ -n "${SHELLWING_E2E_KEEP:-}" ]]; then
		echo "kept: containers and networks $run-*, files in $work"
		return
	fi
	docker rm -f $(docker ps -aq --filter "name=^$run-") >/dev/null 2>&1 || true
	docker network rm "$run-edge" "$run-inner" >/dev/null 2>&1 || true
	rm -rf "$work"
}
trap cleanup EXIT

# Keys for this run: three kinds the servers accept as authorized keys, one
# they accept only through its certificate, and the two CAs.
key() { ssh-keygen -q -N '' -C "shellwing-e2e-$1" "${@:2}" -f "$work/$1"; }
key id_ed25519 -t ed25519
key id_rsa -t rsa -b 3072
key id_ecdsa -t ecdsa -b 256
key id_cert -t ed25519
key user_ca -t ed25519
key host_ca -t ed25519
ssh-keygen -q -s "$work/user_ca" -I e2e -n tester -V -5m:+2h "$work/id_cert.pub"
mkdir "$work/server" "$work/home"
cat "$work"/id_{ed25519,rsa,ecdsa}.pub >"$work/server/authorized_keys"
cp "$work/user_ca.pub" "$work/host_ca" "$work/server/"
LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c 24 >"$work/server/password" || true
# Security keys: software ones (e2e/sk), which only its own ssh-agent can sign
# with. The servers that know this kind of key accept them.
mkdir "$work/sk"
docker build -q -t shellwing-e2e-sk e2e/sk >/dev/null
docker run --rm -e E2E_UID="$(id -u)" -v "$work/sk:/sk" shellwing-e2e-sk generate
cat "$work"/sk/id_*_sk.pub >>"$work/server/authorized_keys"
# The client trusts each host through the host CA, as an organization would.
echo "@cert-authority * $(cat "$work/host_ca.pub")" >"$work/known_hosts"

image() {
	docker build -q -t "shellwing-e2e-sshd:$1" --build-arg "BASE=$2" e2e/openssh >/dev/null
}
start() {
	local name=$1 tag=$2 network=$3
	shift 3
	docker run -d --name "$run-$name" --hostname "$name" \
		--network "$network" --network-alias "$name" \
		-v "$work/server:/e2e:ro" "$@" "shellwing-e2e-sshd:$tag" >/dev/null
}
ready() {
	local c="$run-$1"
	for _ in $(seq 120); do
		docker logs "$c" 2>&1 | grep -q 'Server listening' && return 0
		sleep 0.5
	done
	echo "$1 did not start:" >&2
	docker logs "$c" >&2
	return 1
}

docker network create "$run-edge" >/dev/null
docker network create --internal "$run-inner" >/dev/null
image bastion "$bastion_image"
# It refuses X11 forwarding, so a test can see that reported.
start bastion bastion "$run-edge" -p 127.0.0.1::22 -e E2E_X11_FORWARDING=no
docker network connect "$run-inner" "$run-bastion"
labels=()
for t in "${targets[@]}"; do
	label=${t%%=*}
	image "$label" "${t#*=}"
	start "$label" "$label" "$run-inner"
	labels+=("$label")
done
ready bastion
for label in "${labels[@]}"; do ready "$label"; done

docker run -d --name "$run-sk" -e E2E_UID="$(id -u)" -v "$work/sk:/sk" shellwing-e2e-sk agent >/dev/null
for _ in $(seq 60); do
	[[ -S "$work/sk/agent.sock" ]] && break
	sleep 0.5
done

# An X server for X11 forwarding, where this machine has xauth to use it: its
# TCP port published, and DISPLAY the number that port stands for.
if command -v xauth >/dev/null; then
	mkdir "$work/x11"
	od -An -tx1 -N16 /dev/urandom | tr -d ' \n' >"$work/x11/cookie"
	docker build -q -t shellwing-e2e-xvfb e2e/xvfb >/dev/null
	docker run -d --name "$run-xvfb" --network "$run-edge" -p 127.0.0.1::6099 \
		-v "$work/x11:/e2e:ro" shellwing-e2e-xvfb >/dev/null
	for _ in $(seq 60); do
		docker exec "$run-xvfb" env XAUTHORITY=/tmp/auth xdpyinfo -display :99 >/dev/null 2>&1 && break
		sleep 0.5
	done
	x_port=$(docker port "$run-xvfb" 6099/tcp | head -n1)
	x_port=${x_port##*:}
	export DISPLAY="127.0.0.1:$((x_port - 6000))" XAUTHORITY="$work/x11/auth"
	xauth -f "$XAUTHORITY" add "$DISPLAY" MIT-MAGIC-COOKIE-1 "$(cat "$work/x11/cookie")" 2>/dev/null
	export SHELLWING_E2E_X11_COOKIE
	SHELLWING_E2E_X11_COOKIE=$(cat "$work/x11/cookie")
else
	unset DISPLAY XAUTHORITY
fi

export SHELLWING_E2E_DIR="$work"
export SHELLWING_E2E_BASTION
SHELLWING_E2E_BASTION=$(docker port "$run-bastion" 22/tcp | head -n1)
export SHELLWING_E2E_TARGETS
SHELLWING_E2E_TARGETS=$(IFS=,; echo "${labels[*]}")
# For SendEnv to send.
export SHELLWING_E2E_SENT="sent by this machine"
# Nothing of this machine's own: no ~/.ssh keys to offer, no agent.
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export HOME="$work/home"
unset SSH_AUTH_SOCK
cargo test -p shellwing-ssh --test openssh -- --test-threads=1 "$@"
