#!/bin/bash

set -e -o pipefail

WORKSPACE_DIR=$(basename "$(pwd)")
REDIS_DB=$(( ($(echo "$WORKSPACE_DIR" | cksum | cut -d' ' -f1) % 255) + 1 ))

echo "Setting up workspace: $WORKSPACE_DIR"
echo "Redis database: $REDIS_DB"

ENV_FILE=".env.test"

if [ -f "$ENV_FILE" ]; then
  echo "$ENV_FILE already exists, skipping"
else
  echo "Creating $ENV_FILE..."
  echo "REDIS_URL=redis://localhost:6379/${REDIS_DB}" > "$ENV_FILE"
fi

echo ""
echo "Workspace setup complete!"
echo "  Redis DB: $REDIS_DB"
