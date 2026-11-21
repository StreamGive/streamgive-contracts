# Upgrade runbook

Operational checklist for running `upgrade` on a deployed `donation-vault`
or `ngo-registry`. The rules this checklist follows — storage-layout safety,
error-code stability, and what counts as a breaking change — are in
[UPGRADE_POLICY.md](UPGRADE_POLICY.md); read that first if the change
touches a stored type or an existing entry point's shape.

## 1. Build and verify the hash

```bash
cargo build --workspace --target wasm32v1-none --release
cd target/wasm32v1-none/release
sha256sum donation_vault.wasm ngo_registry.wasm
```

Record the hash(es) in the pull request and in the release notes. Anyone
verifying the deployed code compares this hash against
`stellar contract fetch --id <contract-id>` run through the same command.

## 2. Check storage compatibility

Before the new code is deployed, confirm against the table in
[UPGRADE_POLICY.md §2](UPGRADE_POLICY.md#2-storage-layout-changes-across-an-upgrade):

- Does this change add, remove, rename, or retype a field of `Stream`,
  `StreamStatus`, `Ngo`, or any `DataKey` variant's payload?
- If yes, it must ship with a versioned-key migration or a `migrate` entry
  point (policy §2) and a test that reads an old-layout entry with the new
  code — not as a plain upgrade.

## 3. Deploy

1. `pause` the vault. (`ngo-registry` has no pause; a registry upgrade can
   skip this step.)
2. Call `upgrade` with the verified Wasm hash:
   ```bash
   stellar contract invoke --id <contract-id> --network testnet \
     --source "$STELLAR_SOURCE_ACCOUNT" \
     -- upgrade --new_wasm_hash <hash>
   ```
3. If the release includes a migration, run `migrate` until it reports
   completion.
4. Spot-check with read-only calls (`get_config`, `get_stream` on an old
   and a newly-created stream; `get_ngo` for the registry).
5. `unpause`.
6. Update `deployments.json` and the release notes with the new hash and
   date.

Deploy to testnet first. For a breaking change, wait at least 14 days
before repeating these steps on mainnet (policy §4) unless the release
fixes an actively exploitable vulnerability.

## 4. Announce

Watch for the `UpgradedEvent` both contracts emit — it is the on-chain
signal that the new code is live. Follow policy §4 for changelog and
release-note requirements.

## 5. Rollback plan

`upgrade` replaces the Wasm in place; there is no separate "revert" entry
point. Rolling back means upgrading again, to the previous Wasm hash:

1. Keep the previous release's Wasm hash and the artifact itself (CI
   uploads it under `contracts-wasm`, and `deployments.json` records the
   hash) until the new release has been stable in production for a while.
2. If the new code misbehaves, `pause` immediately (vault only) to stop
   fund movement while the new storage layout, if any, is still in the
   state the broken code produced.
3. Call `upgrade` again with the previous Wasm hash. This only restores
   old *code*; it does not undo storage already written by the broken
   release.
4. If the broken release ran a `migrate` step or wrote new-layout entries,
   rolling back the code alone leaves those entries in a shape the old
   code cannot read. In that case, a straight rollback is not safe —
   fix forward instead (ship a corrected new release), or restore the
   affected entries from an indexer snapshot taken before the migration
   ran, before invoking `upgrade` with the old hash.
5. Re-run step 3's read-only spot checks before `unpause`.

Because of step 4, decide *before* deploying a migration-carrying release
whether a plain hash-rollback would actually be safe for that specific
change — if not, say so in the pull request alongside the migration.
