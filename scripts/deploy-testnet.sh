#!/usr/bin/env bash
# Builds both contracts, deploys them to Stellar testnet, initializes each
# with the deploying identity as admin, and writes the resulting contract
# IDs to deployments.json at the repo root.
#
# Requires the `stellar` CLI (https://developers.stellar.org/docs/tools/cli),
# `node` (to update deployments.json), and a funded testnet identity. Run
# from the repo root:
#
#   STELLAR_SOURCE_ACCOUNT=my-testnet-identity ./scripts/deploy-testnet.sh
#
# Like deploy-mainnet.sh, this refuses to overwrite an existing testnet entry:
# a re-run deploys fresh contracts and orphans the recorded ones. Pass
# --force to overwrite it deliberately.

set -euo pipefail

NETWORK="testnet"
TESTNET_PASSPHRASE="Test SDF Network ; September 2015"
SOURCE_ACCOUNT="${STELLAR_SOURCE_ACCOUNT:?Set STELLAR_SOURCE_ACCOUNT to a funded testnet identity name}"
WASM_DIR="target/wasm32v1-none/release"
DEPLOYMENTS_FILE="deployments.json"

usage() {
  echo "Usage: $0 [--force]" >&2
  echo "  --force   overwrite an existing testnet entry in $DEPLOYMENTS_FILE" >&2
}

FORCE=0
for arg in "$@"; do
  case "$arg" in
    --force) FORCE=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Error: unknown argument '$arg'." >&2
      usage
      exit 1
      ;;
  esac
done

# Fail fast, before anything is built or deployed, if the tools this script
# depends on aren't there — otherwise a missing target can be discovered
# only after ngo-registry has already deployed, leaving a half-finished
# run behind.
check_prerequisites() {
  if ! command -v stellar >/dev/null 2>&1; then
    echo "Error: the 'stellar' CLI is not installed or not on PATH." >&2
    echo "See https://developers.stellar.org/docs/tools/cli for install instructions." >&2
    exit 1
  fi

  if ! command -v node >/dev/null 2>&1; then
    echo "Error: 'node' is not installed or not on PATH; it is needed to update $DEPLOYMENTS_FILE." >&2
    exit 1
  fi

  if ! command -v rustup >/dev/null 2>&1; then
    echo "Error: 'rustup' is not installed or not on PATH; can't verify the wasm32v1-none target." >&2
    exit 1
  fi

  if ! rustup target list --installed | grep -qx "wasm32v1-none"; then
    echo "Error: the wasm32v1-none target is not installed." >&2
    echo "Install it with: rustup target add wasm32v1-none" >&2
    exit 1
  fi
}

# Never clobber a recorded testnet deployment: re-running would deploy fresh
# contracts and orphan the ones users may already be using. Unlike the
# "mainnet" entry, which deploy-mainnet.sh nests under a key, the testnet
# entry lives at the top level of the file, so its presence is marked by a
# top-level "contracts" object.
check_no_existing_deployment() {
  [ -f "$DEPLOYMENTS_FILE" ] || return 0

  local status=0
  DEPLOYMENTS_FILE="$DEPLOYMENTS_FILE" node -e '
    const fs = require("fs");
    let data;
    try {
      data = JSON.parse(fs.readFileSync(process.env.DEPLOYMENTS_FILE, "utf8"));
    } catch (e) {
      console.error("Error: could not parse " + process.env.DEPLOYMENTS_FILE + ": " + e.message);
      process.exit(2);
    }
    process.exit(data.contracts ? 1 : 0);
  ' || status=$?

  if [ "$status" -eq 1 ]; then
    if [ "$FORCE" -eq 1 ]; then
      echo "Warning: overwriting the existing testnet entry in $DEPLOYMENTS_FILE (--force)." >&2
    else
      echo "Error: $DEPLOYMENTS_FILE already has a testnet entry; refusing to overwrite it." >&2
      echo "If you really mean to redeploy, remove that entry by hand first, or pass --force." >&2
      exit 1
    fi
  elif [ "$status" -ne 0 ]; then
    exit 1
  fi
}

check_prerequisites
check_no_existing_deployment

# The admin is deliberately separate from the account paying the deploy
# fees. The admin must be a wallet a human can actually sign with, since
# approve_ngo and friends are driven from the browser admin panel — a
# CLI-only deployer key cannot do that. Set STELLAR_ADMIN_ADDRESS to that
# wallet; it falls back to the deployer for a throwaway local deploy.
#
# Resolved to a G-address up front either way: the CLI takes an identity
# name for --source, but an Address-typed *argument* like init's --admin is
# not guaranteed to resolve the same way, and init can only ever be called
# once per contract — so pass something unambiguous.
ADMIN_ADDRESS="${STELLAR_ADMIN_ADDRESS:-$(stellar keys address "$SOURCE_ACCOUNT")}"
echo "Admin address: $ADMIN_ADDRESS"

# Optional vault configuration environment variables:
#   STELLAR_REGISTRY_ADDRESS - Address of ngo-registry (C... or G...) or "auto" to use newly deployed registry. Default: unset.
#   STELLAR_TREASURY_ADDRESS - Address of fee recipient treasury (C... or G...). Default: unset.
#   STELLAR_FEE_BPS          - Global protocol fee in basis points (0-1000). Default: unset (0).
#                              Requires STELLAR_TREASURY_ADDRESS if > 0.
#   STELLAR_MIN_DEPOSIT      - Minimum stream deposit in stroops (integer >= 0). Default: unset (0).

validate_address() {
  local val="$1"
  local var_name="$2"
  if ! [[ "$val" =~ ^[GC][A-Z2-7]{55}$ ]]; then
    echo "Error: $var_name is not a valid Stellar address (G... or C...): $val" >&2
    exit 1
  fi
}

validate_config_inputs() {
  if [ -n "${STELLAR_ADMIN_ADDRESS:-}" ]; then
    validate_address "$ADMIN_ADDRESS" "STELLAR_ADMIN_ADDRESS"
  fi

  if [ -n "${STELLAR_REGISTRY_ADDRESS:-}" ] && [ "$STELLAR_REGISTRY_ADDRESS" != "auto" ]; then
    validate_address "$STELLAR_REGISTRY_ADDRESS" "STELLAR_REGISTRY_ADDRESS"
  fi

  if [ -n "${STELLAR_TREASURY_ADDRESS:-}" ]; then
    validate_address "$STELLAR_TREASURY_ADDRESS" "STELLAR_TREASURY_ADDRESS"
  fi

  if [ -n "${STELLAR_FEE_BPS:-}" ]; then
    if ! [[ "$STELLAR_FEE_BPS" =~ ^[0-9]+$ ]]; then
      echo "Error: STELLAR_FEE_BPS must be a non-negative integer: $STELLAR_FEE_BPS" >&2
      exit 1
    fi
    if [ "$STELLAR_FEE_BPS" -gt 1000 ]; then
      echo "Error: STELLAR_FEE_BPS cannot exceed 1000 (10% max fee): $STELLAR_FEE_BPS" >&2
      exit 1
    fi
    if [ "$STELLAR_FEE_BPS" -gt 0 ] && [ -z "${STELLAR_TREASURY_ADDRESS:-}" ]; then
      echo "Error: Setting STELLAR_FEE_BPS > 0 requires STELLAR_TREASURY_ADDRESS to be set." >&2
      exit 1
    fi
  fi

  if [ -n "${STELLAR_MIN_DEPOSIT:-}" ]; then
    if ! [[ "$STELLAR_MIN_DEPOSIT" =~ ^[0-9]+$ ]]; then
      echo "Error: STELLAR_MIN_DEPOSIT must be a non-negative integer: $STELLAR_MIN_DEPOSIT" >&2
      exit 1
    fi
  fi
}

validate_config_inputs

echo "Building contracts (release, wasm32v1-none)..."
cargo build --workspace --target wasm32v1-none --release

# A contract's wasm hash is sha256 of its built Wasm — the same value the
# network keys the uploaded code by. Recording it in deployments.json lets a
# consumer confirm the deployed bytecode matches a reviewed build.
compute_wasm_hash() {
  local wasm_path="$1"
  if [ ! -f "$wasm_path" ]; then
    echo "Error: expected Wasm at $wasm_path after the build." >&2
    exit 1
  fi
  WASM_PATH="$wasm_path" node -e 'process.stdout.write(require("crypto").createHash("sha256").update(require("fs").readFileSync(process.env.WASM_PATH)).digest("hex"))'
}

NGO_REGISTRY_WASM_HASH=$(compute_wasm_hash "$WASM_DIR/ngo_registry.wasm")
DONATION_VAULT_WASM_HASH=$(compute_wasm_hash "$WASM_DIR/donation_vault.wasm")

# Capture the git commit for deployment verification
GIT_COMMIT=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
if [ "$GIT_COMMIT" != "unknown" ]; then
  echo "Git commit: $GIT_COMMIT"
else
  echo "Warning: could not determine git commit (not a git repository or git not available)"
fi

deploy_contract() {
  local wasm_name="$1"
  stellar contract deploy \
    --wasm "$WASM_DIR/${wasm_name}.wasm" \
    --source "$SOURCE_ACCOUNT" \
    --network "$NETWORK" \
    -- \
    --admin "$ADMIN_ADDRESS"
}

invoke_vault() {
  stellar contract invoke \
    --id "$DONATION_VAULT_ID" \
    --source "$SOURCE_ACCOUNT" \
    --network "$NETWORK" \
    -- "$@"
}

echo "Deploying and initializing ngo-registry (admin: $ADMIN_ADDRESS)..."
NGO_REGISTRY_ID=$(deploy_contract "ngo_registry")
echo "  -> $NGO_REGISTRY_ID"

echo "Deploying and initializing donation-vault (admin: $ADMIN_ADDRESS)..."
DONATION_VAULT_ID=$(deploy_contract "donation_vault")
echo "  -> $DONATION_VAULT_ID"

# Apply optional post-init configuration to the vault
REGISTRY_TO_SET=""
if [ -n "${STELLAR_REGISTRY_ADDRESS:-}" ]; then
  if [ "$STELLAR_REGISTRY_ADDRESS" = "auto" ]; then
    REGISTRY_TO_SET="$NGO_REGISTRY_ID"
  else
    REGISTRY_TO_SET="$STELLAR_REGISTRY_ADDRESS"
  fi
fi

if [ -n "$REGISTRY_TO_SET" ]; then
  echo "Setting vault registry to $REGISTRY_TO_SET..."
  invoke_vault set_registry --registry "$REGISTRY_TO_SET"
else
  echo "Notice: No registry configured (STELLAR_REGISTRY_ADDRESS is unset)."
fi

if [ -n "${STELLAR_TREASURY_ADDRESS:-}" ]; then
  echo "Setting vault treasury to $STELLAR_TREASURY_ADDRESS..."
  invoke_vault set_treasury --treasury "$STELLAR_TREASURY_ADDRESS"
fi

if [ -n "${STELLAR_FEE_BPS:-}" ]; then
  echo "Setting vault fee_bps to $STELLAR_FEE_BPS..."
  invoke_vault set_fee_bps --fee_bps "$STELLAR_FEE_BPS"
fi

if [ -n "${STELLAR_MIN_DEPOSIT:-}" ]; then
  echo "Setting vault min_deposit to $STELLAR_MIN_DEPOSIT..."
  invoke_vault set_min_deposit --min_deposit "$STELLAR_MIN_DEPOSIT"
fi

echo "Verifying vault admin..."
ACTUAL_ADMIN=$(invoke_vault admin | tr -d '"')
if [ "$ACTUAL_ADMIN" != "$ADMIN_ADDRESS" ]; then
  echo "Error: Vault admin verification failed! Expected: $ADMIN_ADDRESS, got: $ACTUAL_ADMIN" >&2
  exit 1
fi
echo "  -> Admin verified: $ACTUAL_ADMIN"

echo "Reading back final vault configuration..."
ACTUAL_REGISTRY=$(invoke_vault registry | tr -d '"')
CONFIG_RAW=$(invoke_vault get_config)

echo "======================================================================"
echo "Final Donation Vault Configuration:"
echo "  Admin (verified via admin(); not exposed by get_config): $ACTUAL_ADMIN"
echo "  Registry (verified via registry()):                      $ACTUAL_REGISTRY"
echo "  Configuration (via get_config):"
echo "$CONFIG_RAW"
echo "======================================================================"

# deploy-mainnet.sh records its deployment under a "mainnet" key in this same
# file; rewriting the file must not lose it, so carry it over. The entry is
# given the same field set as the "mainnet" one (network_passphrase and the
# wasm hashes) so an indexer can read either without special-casing, and the
# write is validated and renamed into place so a failure can't truncate the
# file.
trap 'rm -f "$DEPLOYMENTS_FILE.tmp"' EXIT

DEPLOYMENTS_FILE="$DEPLOYMENTS_FILE" \
NETWORK="$NETWORK" \
DEPLOYED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
ADMIN_ADDRESS="$ADMIN_ADDRESS" \
NETWORK_PASSPHRASE="$TESTNET_PASSPHRASE" \
NGO_REGISTRY_ID="$NGO_REGISTRY_ID" \
DONATION_VAULT_ID="$DONATION_VAULT_ID" \
NGO_REGISTRY_WASM_HASH="$NGO_REGISTRY_WASM_HASH" \
DONATION_VAULT_WASM_HASH="$DONATION_VAULT_WASM_HASH" \
GIT_COMMIT="$GIT_COMMIT" \
  node -e '
    const fs = require("fs");
    const file = process.env.DEPLOYMENTS_FILE;
    const existing = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, "utf8")) : {};
    const entry = {
      network: process.env.NETWORK,
      network_passphrase: process.env.NETWORK_PASSPHRASE,
      deployed_at: process.env.DEPLOYED_AT,
      admin: process.env.ADMIN_ADDRESS,
      contracts: {
        "ngo-registry": process.env.NGO_REGISTRY_ID,
        "donation-vault": process.env.DONATION_VAULT_ID,
      },
      wasm_hashes: {
        "ngo-registry": process.env.NGO_REGISTRY_WASM_HASH,
        "donation-vault": process.env.DONATION_VAULT_WASM_HASH,
      },
      git_commit: process.env.GIT_COMMIT,
    };
    if (existing.mainnet) entry.mainnet = existing.mainnet;
    const tmp = file + ".tmp";
    fs.writeFileSync(tmp, JSON.stringify(entry, null, 2) + "\n");
    JSON.parse(fs.readFileSync(tmp, "utf8"));
    fs.renameSync(tmp, file);
  '

echo "Wrote $DEPLOYMENTS_FILE"
