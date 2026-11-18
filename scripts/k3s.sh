#!/usr/bin/env bash
# A throwaway Kubernetes (k3s) in Docker, for the Kubernetes end-to-end test of the server.
#
#   scripts/k3s.sh up           start it and write .k3s/kubeconfig
#   scripts/k3s.sh down         remove it and .k3s/
#   scripts/k3s.sh kubeconfig   print the path of the kubeconfig
#
# It never reads or writes ~/.kube, and installs nothing on the host: it only runs one
# container. SPEEDYSPEC_K3S_PORT picks the loopback port the API is published on.

set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Pinned, so the test means the same thing every time.
image="rancher/k3s:v1.34.13-k3s1"
name="speedyspec-server-k3s"
# Marks the container as one this script made.
label="speedyspec-server.k3s=1"
port="${SPEEDYSPEC_K3S_PORT:-16444}"
state="$repo/.k3s"
kubeconfig="$state/kubeconfig"

mine() {
    [ -n "$(docker ps -a --quiet --filter "name=^${name}\$" --filter "label=${label}")" ]
}

exists() {
    [ -n "$(docker ps -a --quiet --filter "name=^${name}\$")" ]
}

case "${1:-}" in
    up)
        if exists && ! mine; then
            echo "scripts/k3s.sh: a container named $name exists and this script did not create it" >&2
            exit 1
        fi
        if exists; then
            docker rm --force "$name" >/dev/null
        fi
        mkdir -p "$state"
        docker run --detach --name "$name" --label "$label" --privileged \
            --publish "127.0.0.1:${port}:6443" \
            "$image" server --disable traefik --disable metrics-server --tls-san 127.0.0.1 >/dev/null
        # Ready when the API answers and the node can schedule pods.
        for _ in $(seq 1 120); do
            if docker exec "$name" kubectl get --raw /readyz >/dev/null 2>&1 \
                && docker exec "$name" kubectl get nodes 2>/dev/null | grep -q ' Ready ' \
                && docker exec "$name" kubectl get serviceaccount default >/dev/null 2>&1; then
                docker exec "$name" cat /etc/rancher/k3s/k3s.yaml \
                    | sed "s#https://127.0.0.1:6443#https://127.0.0.1:${port}#" >"$kubeconfig"
                chmod 600 "$kubeconfig"
                echo "$kubeconfig"
                exit 0
            fi
            sleep 2
        done
        echo "scripts/k3s.sh: k3s did not become ready; its log follows" >&2
        docker logs --tail 40 "$name" >&2 || true
        exit 1
        ;;
    down)
        if exists && ! mine; then
            echo "scripts/k3s.sh: a container named $name exists and this script did not create it" >&2
            exit 1
        fi
        if exists; then
            docker rm --force --volumes "$name" >/dev/null
        fi
        rm -rf "$state"
        ;;
    kubeconfig)
        echo "$kubeconfig"
        ;;
    *)
        echo "usage: scripts/k3s.sh up | down | kubeconfig" >&2
        exit 2
        ;;
esac
