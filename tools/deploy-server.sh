#!/bin/sh
# Builds the game server's image and (re)deploys it as the `lilienstern` Swarm stack, on a
# single-node swarm with Traefik (see deploy/server/stack.yml). Run from anywhere:
#   sh tools/deploy-server.sh
set -e
cd "$(dirname "$0")/.."
tag="lilienstern-server:$(git rev-parse --short HEAD)$(git diff --quiet -- crates || echo -dirty)"
docker build -f deploy/server/Dockerfile -t "$tag" -t lilienstern-server:latest .
LIL_IMAGE="$tag" docker stack deploy --resolve-image never -c deploy/server/stack.yml lilienstern
echo "deployed $tag"
