#!/bin/bash

set -e -o pipefail

WORKSPACE_DIR=$(basename "$(pwd)")
REDIS_DB=$(( ($(echo "$WORKSPACE_DIR" | cksum | cut -d' ' -f1) % 255) + 1 ))
REDIS_CONTAINER="redis"

echo "Archiving workspace: $WORKSPACE_DIR"

# Flush Redis database
if docker ps --format '{{.Names}}' | grep -q "$REDIS_CONTAINER"; then
  echo "Flushing Redis database $REDIS_DB..."
  docker exec "$REDIS_CONTAINER" redis-cli -n "$REDIS_DB" FLUSHDB
else
  echo "Redis container not running, skipping Redis cleanup"
fi

echo "Workspace archive complete!"
