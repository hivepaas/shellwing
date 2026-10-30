#!/bin/sh
# Starts sshd as the tests expect it, from what scripts/e2e_openssh.sh puts
# in /e2e (read-only): the user's password and keys, the user CA it trusts,
# and the host CA that signs this host's key.
set -eu
name=$(hostname)

ssh-keygen -A >/dev/null
# The client trusts the host CA (@cert-authority), not each host's key.
cp /e2e/host_ca /tmp/host_ca
chmod 600 /tmp/host_ca
ssh-keygen -q -s /tmp/host_ca -I "$name" -h -n "$name,127.0.0.1,localhost" \
    /etc/ssh/ssh_host_ed25519_key.pub
rm /tmp/host_ca

if command -v useradd >/dev/null; then
    useradd -m -s /bin/sh tester
else
    adduser -D -s /bin/sh tester
fi
echo "tester:$(cat /e2e/password)" | chpasswd
mkdir -p /home/tester/.ssh
cp /e2e/authorized_keys /home/tester/.ssh/authorized_keys
chown -R tester /home/tester/.ssh
chmod 700 /home/tester/.ssh
chmod 600 /home/tester/.ssh/authorized_keys

# A whole config of its own: the oldest servers here (OpenSSH 8.0) have no
# Include, and each distribution's own file differs.
mkdir -p /run/sshd
cat >/etc/ssh/e2e_sshd_config <<EOF
Port 22
HostKey /etc/ssh/ssh_host_ed25519_key
HostKey /etc/ssh/ssh_host_ecdsa_key
HostKey /etc/ssh/ssh_host_rsa_key
HostCertificate /etc/ssh/ssh_host_ed25519_key-cert.pub
PermitRootLogin no
PasswordAuthentication yes
PubkeyAuthentication yes
AuthorizedKeysFile .ssh/authorized_keys
TrustedUserCAKeys /e2e/user_ca.pub
AllowTcpForwarding yes
AllowAgentForwarding yes
AcceptEnv LANG LC_* SHELLWING_*
X11Forwarding ${E2E_X11_FORWARDING:-yes}
# Where IPv6 is missing, sshd finds no X11 display socket otherwise.
AddressFamily inet
Subsystem sftp internal-sftp
PidFile /run/sshd.pid
LogLevel VERBOSE
EOF
# OpenSSH 9.8 and later shut out an address that failed to sign in, for a
# while: the tests fail on purpose, all from one address (Docker's gateway).
if /usr/sbin/sshd -t -f /etc/ssh/e2e_sshd_config -o PerSourcePenalties=no 2>/dev/null; then
    echo "PerSourcePenalties no" >>/etc/ssh/e2e_sshd_config
fi
exec /usr/sbin/sshd -D -e -f /etc/ssh/e2e_sshd_config
