#!/usr/bin/env bash
# Builds both contracts, deploys them to Stellar MAINNET, initializes each
# with the admin wallet, and records the resulting contract IDs under the
# "mainnet" key of deployments.json at the repo root.
#
# Mainnet deploys spend real funds and can't be undone (init can only be
# called once, and ngo-registry has no admin transfer), so this is stricter
# than deploy-testnet.sh:
#
#   * it refuses to run without --confirm;
#   * it always talks to the Public network: the passphrase is pinned here,
#     and a conflicting STELLAR_NETWORK_PASSPHRASE aborts the run;
#   * STELLAR_ADMIN_ADDRESS is mandatory (no fallback to the deployer key)
#     and must be a valid G-address;
#   * it refuses to overwrite an existing "mainnet" entry in deployments.json.
#
# Requires the `stellar` CLI (https://developers.stellar.org/docs/tools/cli),
# `node` (to update deployments.json), and a funded mainnet identity. Run from
# the repo root:
#
#   STELLAR_SOURCE_ACCOUNT=my-mainnet-identity \
#   STELLAR_ADMIN_ADDRESS=G... \
#   STELLAR_RPC_URL=https://<your-mainnet-rpc> \
#     ./scripts/deploy-mainnet.sh --confirm

# Optional vault configuration environment variables:
#   STELLAR_REGISTRY_ADDRESS - Address of ngo-registry (C... or G...) or "auto" to use newly deployed registry. Default: unset.
#                              If unset, --allow-no-registry is required (threat model T-21).
#   STELLAR_TREASURY_ADDRESS - Treasury address for protocol fee collection (C... or G...). Default: unset.
#   STELLAR_FEE_BPS          - Global protocol fee in basis points (0-1000). Default: unset (0).
#                              Requires STELLAR_TREASURY_ADDRESS if > 0.
#   STELLAR_MIN_DEPOSIT      - Minimum stream deposit in stroops (integer >= 0). Default: unset (0).

set -euo pipefail

NETWORK="mainnet"
MAINNET_PASSPHRASE="Public Global Stellar Network ; September 2015"
WASM_DIR="target/wasm32v1-none/release"
DEPLOYMENTS_FILE="deployments.json"

usage() {
  echo "Usage: $0 --confirm [--allow-no-registry]" >&2
  echo "  --confirm             acknowledge that this deploys to Stellar MAINNET with real funds" >&2
  echo "  --allow-no-registry   explicitly opt in to deploying without an NGO registry (threat model T-21)" >&2
}

CONFIRMED=0
ALLOW_NO_REGISTRY=0
for arg in "$@"; do
  case "$arg" in
    --confirm) CONFIRMED=1 ;;
    --allow-no-registry) ALLOW_NO_REGISTRY=1 ;;
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

if [ "$CONFIRMED" -ne 1 ]; then
  echo "Error: refusing to deploy to MAINNET without --confirm." >&2
  usage
  exit 1
fi

SOURCE_ACCOUNT="${STELLAR_SOURCE_ACCOUNT:?Set STELLAR_SOURCE_ACCOUNT to a funded mainnet identity name}"
ADMIN_ADDRESS="${STELLAR_ADMIN_ADDRESS:?Set STELLAR_ADMIN_ADDRESS to the mainnet admin wallet (a G... address)}"
RPC_URL="${STELLAR_RPC_URL:?Set STELLAR_RPC_URL to a mainnet Soroban RPC endpoint}"

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

# The passphrase is what actually distinguishes networks: a transaction
# signed for one passphrase is invalid on another. It is pinned above and
# passed explicitly on every call, so the only way to get it wrong is an
# environment override — reject that rather than silently ignoring it.
check_network_passphrase() {
  local from_env="${STELLAR_NETWORK_PASSPHRASE:-}"
  if [ -n "$from_env" ] && [ "$from_env" != "$MAINNET_PASSPHRASE" ]; then
    echo "Error: STELLAR_NETWORK_PASSPHRASE is set to a non-mainnet value:" >&2
    echo "  $from_env" >&2
    echo "Expected: $MAINNET_PASSPHRASE" >&2
    exit 1
  fi
}

check_admin_address() {
  if ! [[ "$ADMIN_ADDRESS" =~ ^G[A-Z2-7]{55}$ ]]; then
    echo "Error: STELLAR_ADMIN_ADDRESS is not a valid Stellar account address (G...): $ADMIN_ADDRESS" >&2
    exit 1
  fi
}

# Never clobber a recorded mainnet deployment: re-running would deploy fresh
# contracts and orphan the ones users may already be using.
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
    process.exit(data.mainnet ? 1 : 0);
  ' || status=$?

  if [ "$status" -eq 1 ]; then
    echo "Error: $DEPLOYMENTS_FILE already has a \"mainnet\" entry; refusing to overwrite it." >&2
    echo "If you really mean to redeploy, remove that entry by hand first." >&2
    exit 1
  elif [ "$status" -ne 0 ]; then
    exit 1
  fi
}

# A contract's wasm hash is sha256 of its built Wasm — the same value the
# network keys the uploaded code by. Recorded in deployments.json so a
# consumer can confirm the deployed bytecode matches a reviewed build, and so
# the "mainnet" and top-level testnet entries share one shape.
compute_wasm_hash() {
  local wasm_path="$1"
  if [ ! -f "$wasm_path" ]; then
    echo "Error: expected Wasm at $wasm_path after the build." >&2
    exit 1
  fi
  WASM_PATH="$wasm_path" node -e 'process.stdout.write(require("crypto").createHash("sha256").update(require("fs").readFileSync(process.env.WASM_PATH)).digest("hex"))'
}

validate_address() {
  local val="$1"
  local var_name="$2"
  if ! [[ "$val" =~ ^[GC][A-Z2-7]{55}$ ]]; then
    echo "Error: $var_name is not a valid Stellar address (G... or C...): $val" >&2
    exit 1
  fi
}

validate_config_inputs() {
  if [ -n "${STELLAR_REGISTRY_ADDRESS:-}" ]; then
    if [ "$STELLAR_REGISTRY_ADDRESS" != "auto" ]; then
      validate_address "$STELLAR_REGISTRY_ADDRESS" "STELLAR_REGISTRY_ADDRESS"
    fi
  else
    if [ "$ALLOW_NO_REGISTRY" -ne 1 ]; then
      echo "Error: STELLAR_REGISTRY_ADDRESS is not set." >&2
      echo "Per THREAT_MODEL.md (T-21), set_registry is a mandatory mainnet deployment step" >&2
      echo "to ensure unverified NGOs cannot receive donation streams. To explicitly deploy" >&2
      echo "without a registry, pass --allow-no-registry." >&2
      exit 1
    else
      echo "WARNING: Deploying to MAINNET with NO registry configured (--allow-no-registry specified)!" >&2
      echo "         The vault will accept donation streams to any unverified address until set_registry is called." >&2
    fi
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

check_prerequisites
check_network_passphrase
check_admin_address
check_no_existing_deployment
validate_config_inputs

echo "About to deploy to Stellar MAINNET."
echo "  Deployer identity: $SOURCE_ACCOUNT"
echo "  Admin address:     $ADMIN_ADDRESS"
echo "  RPC URL:           $RPC_URL"

echo "Building contracts (release, wasm32v1-none)..."
cargo build --workspace --target wasm32v1-none --release

# Computed before the ERR trap is armed: a missing artifact is a build
# failure, not a part-way deploy.
NGO_REGISTRY_WASM_HASH=$(compute_wasm_hash "$WASM_DIR/ngo_registry.wasm")
DONATION_VAULT_WASM_HASH=$(compute_wasm_hash "$WASM_DIR/donation_vault.wasm")

# Capture the git commit for deployment verification
GIT_COMMIT=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
if [ "$GIT_COMMIT" != "unknown" ]; then
  echo "Git commit: $GIT_COMMIT"
else
  echo "Warning: could not determine git commit (not a git repository or git not available)"
fi

NGO_REGISTRY_ID=""
DONATION_VAULT_ID=""

# If anything fails after a contract is already on-chain, fail loudly and
# print what was deployed. Because contracts are initialized atomically via
# __constructor at deploy time, deployed contracts are never left uninitialized
# or claimable, but $DEPLOYMENTS_FILE has not yet recorded them.
report_partial_deploy() {
  echo >&2
  echo "======================================================================" >&2
  echo "ERROR: Mainnet deploy failed part-way! $DEPLOYMENTS_FILE was NOT updated." >&2
  echo "Contracts are initialized atomically with admin $ADMIN_ADDRESS at deploy" >&2
  echo "time and cannot be claimed by third parties, but manual recovery or" >&2
  echo "record-keeping is required for any deployed contracts below:" >&2
  echo "  ngo-registry:   ${NGO_REGISTRY_ID:-<not deployed>}" >&2
  echo "  donation-vault: ${DONATION_VAULT_ID:-<not deployed>}" >&2
  echo "======================================================================" >&2
}
trap report_partial_deploy ERR

stellar_network_args=(--rpc-url "$RPC_URL" --network-passphrase "$MAINNET_PASSPHRASE")

deploy_contract() {
  local wasm_name="$1"
  stellar contract deploy \
    --wasm "$WASM_DIR/${wasm_name}.wasm" \
    --source "$SOURCE_ACCOUNT" \
    "${stellar_network_args[@]}" \
    -- \
    --admin "$ADMIN_ADDRESS"
}

invoke_vault() {
  stellar contract invoke \
    --id "$DONATION_VAULT_ID" \
    --source "$SOURCE_ACCOUNT" \
    "${stellar_network_args[@]}" \
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
echo "Final Mainnet Donation Vault Configuration:"
echo "  Admin (verified via admin(); not exposed by get_config): $ACTUAL_ADMIN"
echo "  Registry (verified via registry()):                      $ACTUAL_REGISTRY"
echo "  Configuration (via get_config):"
echo "$CONFIG_RAW"
echo "======================================================================"

# Add the mainnet entry next to whatever is already in the file (the
# testnet deployment lives at the top level), leaving that untouched.
DEPLOYMENTS_FILE="$DEPLOYMENTS_FILE" \
NETWORK="$NETWORK" \
DEPLOYED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
ADMIN_ADDRESS="$ADMIN_ADDRESS" \
NETWORK_PASSPHRASE="$MAINNET_PASSPHRASE" \
NGO_REGISTRY_ID="$NGO_REGISTRY_ID" \
DONATION_VAULT_ID="$DONATION_VAULT_ID" \
NGO_REGISTRY_WASM_HASH="$NGO_REGISTRY_WASM_HASH" \
DONATION_VAULT_WASM_HASH="$DONATION_VAULT_WASM_HASH" \
GIT_COMMIT="$GIT_COMMIT" \
  node -e '
    const fs = require("fs");
    const file = process.env.DEPLOYMENTS_FILE;
    const data = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, "utf8")) : {};
    data.mainnet = {
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
    fs.writeFileSync(file, JSON.stringify(data, null, 2) + "\n");
  '

trap - ERR
echo "Wrote mainnet contract IDs to $DEPLOYMENTS_FILE"
