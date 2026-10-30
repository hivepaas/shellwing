#!/bin/sh
set -eu
xauth -f /tmp/auth add :99 MIT-MAGIC-COOKIE-1 "$(cat /e2e/cookie)"
# -noreset: with no other client, it would reset when xauth leaves, and forget
# the limited cookie xauth just had it make. A desktop always has other clients.
exec Xvfb :99 -screen 0 800x600x24 -listen tcp -auth /tmp/auth +extension SECURITY -noreset
