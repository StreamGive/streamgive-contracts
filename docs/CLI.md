# Stellar CLI Cheat Sheet for StreamGive Contracts

Copy-ready `stellar contract invoke` commands for building, deploying, initializing, and operating `ngo-registry` and `donation-vault` on Stellar **testnet**.

---

## 1. Environment Variables & Identity Setup

Export these shell variables once per session so every command below is copy-pasteable:

```bash
export NETWORK="testnet"
export SOURCE="my-testnet-admin"

# Generate and fund a testnet identity via Friendbot (skip if already funded)
stellar keys generate "$SOURCE" --network "$NETWORK" --fund
export ADMIN_ADDR="$(stellar keys address "$SOURCE")"

# Separate donor, NGO, and treasury test identities
stellar keys generate donor --network "$NETWORK" --fund
stellar keys generate ngo --network "$NETWORK" --fund
stellar keys generate treasury --network "$NETWORK" --fund

export DONOR_ADDR="$(stellar keys address donor)"
export NGO_ADDR="$(stellar keys address ngo)"
export TREASURY_ADDR="$(stellar keys address treasury)"

# Native XLM Stellar Asset Contract (SAC) on testnet
export TOKEN_ID="$(stellar contract id asset --asset native --network "$NETWORK")"
```

If `deployments.json` already contains deployed testnet contract IDs, load them with:

```bash
export REGISTRY_ID="$(node -p "require('./deployments.json').testnet.contracts['ngo-registry']")"
export VAULT_ID="$(node -p "require('./deployments.json').testnet.contracts['donation-vault']")"
```

---

## 2. Build, Deploy & Initialize (`init`)

You can run `STELLAR_SOURCE_ACCOUNT="$SOURCE" ./scripts/deploy-testnet.sh` to automate building, deploying, and initializing both contracts, or run each step manually:

```bash
# Build optimized release Wasm binaries
cargo build --target wasm32v1-none --release

# Deploy ngo-registry and donation-vault
export REGISTRY_ID="$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/ngo_registry.wasm \
  --source-account "$SOURCE" \
  --network "$NETWORK")"

export VAULT_ID="$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/donation_vault.wasm \
  --source-account "$SOURCE" \
  --network "$NETWORK")"

# Initialize ngo-registry with ADMIN_ADDR
stellar contract invoke \
  --id "$REGISTRY_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- init \
  --admin "$ADMIN_ADDR"

# Initialize donation-vault with ADMIN_ADDR
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- init \
  --admin "$ADMIN_ADDR"
```

---

## 3. NGO Registration & Approval (`register` / `approve`)

```bash
# NGO self-registers its organization name and optional metadata URI
stellar contract invoke \
  --id "$REGISTRY_ID" \
  --source-account ngo \
  --network "$NETWORK" \
  -- register \
  --owner "$NGO_ADDR" \
  --name "Clean Water Foundation"

# Admin (or designated approver) approves the NGO
stellar contract invoke \
  --id "$REGISTRY_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- approve \
  --owner "$NGO_ADDR"

# Optional: approve with an expiration timestamp (e.g. 1 year from now)
stellar contract invoke \
  --id "$REGISTRY_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- approve_with_expiry \
  --owner "$NGO_ADDR" \
  --expires_at "$(( $(date +%s) + 31536000 ))"

# Verify NGO status (read-only)
stellar contract invoke \
  --id "$REGISTRY_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- is_verified \
  --owner "$NGO_ADDR"
```

---

## 4. Stream Creation (`create_stream`)

```bash
# Create an immediately-accruing stream (deposit: 100 XLM = 1_000_000_000 stroops, rate: 1000 stroops/sec)
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account donor \
  --network "$NETWORK" \
  -- create_stream \
  --donor "$DONOR_ADDR" \
  --ngo "$NGO_ADDR" \
  --token "$TOKEN_ID" \
  --deposit 1000000000 \
  --rate 1000

# Create a stream with a future cliff timestamp (starts accruing 1 hour from now)
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account donor \
  --network "$NETWORK" \
  -- create_stream_with_start \
  --donor "$DONOR_ADDR" \
  --ngo "$NGO_ADDR" \
  --token "$TOKEN_ID" \
  --deposit 1000000000 \
  --rate 1000 \
  --start "$(( $(date +%s) + 3600 ))"

# Inspect stream state and pending accrual (stream_id = 0)
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account donor \
  --network "$NETWORK" \
  -- get_stream \
  --stream_id 0

stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account donor \
  --network "$NETWORK" \
  -- pending_accrual \
  --stream_id 0
```

---

## 5. Withdrawals & Stream Updates (`withdraw` / `top_up` / `cancel_stream`)

```bash
# NGO withdraws accrued funds from stream 0
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account ngo \
  --network "$NETWORK" \
  -- withdraw \
  --stream_id 0

# NGO batch-withdraws across multiple streams
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account ngo \
  --network "$NETWORK" \
  -- withdraw_batch \
  --stream_ids '[0]'

# Donor tops up stream 0 with an additional 50 XLM (500_000_000 stroops)
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account donor \
  --network "$NETWORK" \
  -- top_up \
  --stream_id 0 \
  --amount 500000000

# Donor updates the stream rate to 2000 stroops/sec
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account donor \
  --network "$NETWORK" \
  -- update_rate \
  --stream_id 0 \
  --new_rate 2000

# Donor cancels stream 0 (settles accrued payout to NGO and refunds remainder to donor)
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account donor \
  --network "$NETWORK" \
  -- cancel_stream \
  --stream_id 0
```

---

## 6. Pause, Emergency Rescue & Configuration Changes (`pause` / `set_*`)

```bash
# Link the vault to ngo-registry so only verified NGOs can receive new streams
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- set_registry \
  --registry "$REGISTRY_ID"

# Configure protocol treasury and fee in basis points (e.g. 50 bps = 0.50%, max 500 bps)
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- set_treasury \
  --treasury "$TREASURY_ADDR"

stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- set_fee_bps \
  --fee_bps 50

# Set maximum stream rate cap (e.g. 10_000_000 stroops/sec)
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- set_max_rate \
  --max_rate 10000000

# Pause and unpause the donation vault
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- pause

# Emergency rescue while paused (refunds 100% of remaining stream balance to donor)
stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- rescue_stream \
  --stream_id 0

stellar contract invoke \
  --id "$VAULT_ID" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- unpause
```
