#!/usr/bin/env bash
# Builds both contracts, deploys them to Stellar testnet, initializes each
# with the deploying identity as admin, and writes the resulting contract
# IDs to deployments.json at the repo root.
#
# Requires the `stellar` CLI (https://developers.stellar.org/docs/tools/cli)
# and a funded testnet identity. Run from the repo root:
#
#   STELLAR_SOURCE_ACCOUNT=my-testnet-identity ./scripts/deploy-testnet.sh

set -euo pipefail

NETWORK="testnet"
SOURCE_ACCOUNT="${STELLAR_SOURCE_ACCOUNT:?Set STELLAR_SOURCE_ACCOUNT to a funded testnet identity name}"
WASM_DIR="target/wasm32v1-none/release"
DEPLOYMENTS_FILE="deployments.json"

echo "Building contracts (release, wasm32v1-none)..."
cargo build --workspace --target wasm32v1-none --release

deploy_contract() {
  local wasm_name="$1"
  stellar contract deploy \
    --wasm "$WASM_DIR/${wasm_name}.wasm" \
    --source "$SOURCE_ACCOUNT" \
    --network "$NETWORK"
}

echo "Deploying ngo-registry..."
NGO_REGISTRY_ID=$(deploy_contract "ngo_registry")
echo "  -> $NGO_REGISTRY_ID"

echo "Deploying donation-vault..."
DONATION_VAULT_ID=$(deploy_contract "donation_vault")
echo "  -> $DONATION_VAULT_ID"

echo "Initializing ngo-registry (admin: $SOURCE_ACCOUNT)..."
stellar contract invoke \
  --id "$NGO_REGISTRY_ID" \
  --source "$SOURCE_ACCOUNT" \
  --network "$NETWORK" \
  -- init --admin "$SOURCE_ACCOUNT"

echo "Initializing donation-vault (admin: $SOURCE_ACCOUNT)..."
stellar contract invoke \
  --id "$DONATION_VAULT_ID" \
  --source "$SOURCE_ACCOUNT" \
  --network "$NETWORK" \
  -- init --admin "$SOURCE_ACCOUNT"

cat > "$DEPLOYMENTS_FILE" <<EOF
{
  "network": "$NETWORK",
  "deployed_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "admin": "$SOURCE_ACCOUNT",
  "contracts": {
    "ngo-registry": "$NGO_REGISTRY_ID",
    "donation-vault": "$DONATION_VAULT_ID"
  }
}
EOF

echo "Wrote $DEPLOYMENTS_FILE"
