# Deployment Verification

This document explains how to verify that a deployed contract matches the source code at a specific git commit.

## Overview

The `deployments.json` file records:
- **Contract addresses**: The on-chain addresses of deployed contracts
- **Wasm hashes**: SHA-256 hashes of the compiled contract binaries
- **Git commit**: The exact source code revision used to build the contracts

This allows anyone to independently verify that the deployed code matches reviewed source code.

## Verification Steps

### 1. Check the Deployment Record

Inspect `deployments.json` to find the deployment you want to verify. For testnet:

```json
{
  "network": "testnet",
  "deployed_at": "2026-09-14T21:00:25Z",
  "admin": "GA4Z4GPSO3FKPMQJ3WCMU5D5WE25SDXZS47YDF3J3NWF65ILGCVJVWIJ",
  "contracts": {
    "ngo-registry": "CCPWQBI7FV2CZ7EECOVZ5XHOHK5S7CS6XTBDWPWHBEOKPTIASNGOBQFH",
    "donation-vault": "CDLKBGX3VTFZZBVBNQYHF47PQO6I7UTBYVVQE3BPJDONF6G4U33Z7VLH"
  },
  "wasm_hashes": {
    "ngo-registry": "abc123...",
    "donation-vault": "def456..."
  },
  "git_commit": "a1b2c3d4e5f6..."
}
```

For mainnet, the deployment is nested under the `mainnet` key.

### 2. Check Out the Source Code

Clone the repository and check out the recorded commit:

```bash
git clone https://github.com/StreamGive/streamgive-contracts.git
cd streamgive-contracts
git checkout <git_commit>
```

Replace `<git_commit>` with the commit hash from `deployments.json`.

### 3. Build the Contracts

Build the contracts using the exact release configuration:

```bash
cargo build --workspace --target wasm32v1-none --release
```

This produces the contract binaries in `target/wasm32v1-none/release/`.

### 4. Compute the Wasm Hashes

Compute the SHA-256 hash of each built binary:

```bash
sha256sum target/wasm32v1-none/release/ngo_registry.wasm
sha256sum target/wasm32v1-none/release/donation_vault.wasm
```

On macOS, use `shasum -a 256` instead of `sha256sum`.

### 5. Compare the Hashes

Compare the computed hashes with those recorded in `deployments.json`:

- **Match**: The deployed code matches the source at the recorded commit
- **Mismatch**: The deployment does not match the recorded commit (investigate further)

### 6. Verify On-Chain Code (Optional)

For additional verification, you can fetch the deployed contract's Wasm hash directly from the network and compare it with the recorded hash.

Using the Stellar CLI:

```bash
stellar contract fetch \
  --id <contract_address> \
  --network testnet \
  --out-file fetched.wasm

sha256sum fetched.wasm
```

This hash should match both the locally built binary and the `wasm_hashes` entry in `deployments.json`.

## Reproducible Builds

For fully reproducible builds, use the same Rust toolchain version as the deployment. The Cargo.lock file pins all dependency versions, ensuring consistent builds across different environments.

To check your Rust toolchain version:

```bash
rustc --version
cargo --version
```

## Notes

- **Git commit "unknown"**: If the deployment was built outside a git repository or without git installed, the commit will be recorded as "unknown". In this case, hash verification alone must be relied upon.
- **Deterministic builds**: Rust/Cargo builds with the same source, toolchain, and flags produce identical binaries, making wasm hash comparison reliable.
- **Wasm optimization**: The contracts are built with `opt-level = "z"` and `lto = true` (see `Cargo.toml`), which affects the binary but is consistent across builds.

## Troubleshooting

### Hash mismatch despite correct commit

- Ensure you're using the same Rust toolchain version
- Verify the wasm32v1-none target is installed: `rustup target add wasm32v1-none`
- Check that Cargo.lock has not been modified
- Confirm you're building in release mode with the correct target

### Unable to fetch deployed contract

- Verify the contract address is correct
- Ensure you're querying the correct network (testnet vs mainnet)
- Check that the contract has not been upgraded (the Wasm hash changes after an upgrade)

## See Also

- [Stellar CLI Documentation](https://developers.stellar.org/docs/tools/cli)
- [Soroban Deployment Guide](https://developers.stellar.org/docs/smart-contracts/getting-started/deploy-to-testnet)
- `scripts/deploy-testnet.sh` - Testnet deployment script
- `scripts/deploy-mainnet.sh` - Mainnet deployment script
