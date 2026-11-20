#!/bin/sh
set -eu

ip=${TURN_LISTEN_IP:?Set the TURN IPv4 address}
secret=${REVIEW_PARTY_TURN_SECRET:?Set the TURN signing secret}
case "$ip" in *[!0-9.]*|'') exit 1 ;; esac
case "$secret" in *[!a-f0-9]*|'') exit 1 ;; esac
test "${#secret}" -ge 64

umask 077
config=$(mktemp /tmp/repo-city-turn.XXXXXX)
cat > "$config" <<CONFIG
listening-ip=$ip
relay-ip=$ip
listening-port=3478
min-port=49160
max-port=49223
realm=repo-city.meta-uber-engineer.dev
fingerprint
use-auth-secret
static-auth-secret=$secret
no-cli
no-tls
no-dtls
no-tcp-relay
no-multicast-peers
denied-peer-ip=0.0.0.0-0.255.255.255
denied-peer-ip=10.0.0.0-10.255.255.255
denied-peer-ip=100.64.0.0-100.127.255.255
denied-peer-ip=127.0.0.0-127.255.255.255
denied-peer-ip=169.254.0.0-169.254.255.255
denied-peer-ip=172.16.0.0-172.31.255.255
denied-peer-ip=192.168.0.0-192.168.255.255
denied-peer-ip=198.18.0.0-198.19.255.255
denied-peer-ip=240.0.0.0-255.255.255.255
denied-peer-ip=::1
denied-peer-ip=fc00::-fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff
denied-peer-ip=fe80::-febf:ffff:ffff:ffff:ffff:ffff:ffff:ffff
user-quota=8
total-quota=64
max-bps=64000
bps-capacity=1200000
log-file=stdout
simple-log
pidfile=/tmp/repo-city-turn.pid
CONFIG

exec turnserver -c "$config"
