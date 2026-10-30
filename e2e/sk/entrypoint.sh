#!/bin/sh
# `generate` makes the run's keys in /sk; `agent` serves them from an ssh-agent
# on /sk/agent.sock. Both as the user who runs the tests: ssh-agent answers only
# its own user.
set -eu
adduser -D -u "$E2E_UID" tester
as_tester() { su tester -s /bin/sh -c "$1"; }
provider=/usr/local/lib/sk-dummy.so
case "$1" in
generate)
    for kind in ed25519 ecdsa; do
        as_tester "ssh-keygen -q -t $kind-sk -w $provider -N '' -C shellwing-e2e-$kind-sk -f /sk/id_${kind}_sk"
    done
    ;;
agent)
    as_tester "ssh-agent -a /sk/starting.sock -P '*' >/sk/agent.env"
    as_tester ". /sk/agent.env >/dev/null; for kind in ed25519 ecdsa; do ssh-add -q -S $provider /sk/id_\${kind}_sk; done; ln /sk/starting.sock /sk/agent.sock"
    exec sleep infinity
    ;;
esac
