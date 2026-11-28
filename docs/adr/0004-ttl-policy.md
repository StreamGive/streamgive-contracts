# 0004. Extend storage TTLs eagerly on every state-changing call

- **Status:** Accepted
- **Date:** 2026-11-27 (recorded after the fact)
- **Applies to:** `donation-vault` and `ngo-registry`

## Context

On Soroban, every storage entry has a time to live (TTL), counted in ledgers.
An entry whose TTL runs out is archived. It can be restored, but only at extra
cost and effort, and a contract that reads an archived entry fails. Instance
storage shares one TTL for the whole contract instance, while each persistent
entry has its own. Streams hold donor money and can sit untouched for a long
time, so letting one lapse unnoticed is a real risk.

## Decision

Both contracts bump TTLs on every state-changing path, using constants shared
in the `common` crate, and keep a small set of explicit keep-alive calls.

| Storage | Bump to | Refresh once below | Used for |
| --- | --- | --- | --- |
| Instance | 30 days | 29 days | Admin, configuration, counters, and totals |
| Persistent | 90 days | 89 days | Streams, NGO entries, indexes, per-donor and per-token records |

A day is `DAY_IN_LEDGERS = 17,280`, assuming a 5-second ledger.

- **Instance TTL is extended as a unit** by `extend_instance_ttl` on every
  state-changing entry point.
- **Each persistent entry is extended when it is touched.** A stream gets
  `extend_stream_ttl` on any call that reads or changes it, and an NGO entry
  gets `extend_ngo_ttl`.
- **A cancelled stream is kept longer.** Its TTL is bumped to the normal
  retention plus the admin-configured `cancel_grace_ledgers`, so indexers have a
  window to observe the cancellation before the record can be archived.
- **There are explicit keep-alive calls** for entries that may otherwise go
  untouched, such as `extend_stream` and `touch_ngo`.

[STORAGE.md](../STORAGE.md) lists the TTL and storage type for every key.

## Consequences

- Normal use keeps live data alive. An entry expires only after a long period of
  inactivity, roughly 30 days for instance storage and 90 days for persistent
  entries.
- Every state-changing call pays a small extra cost to extend the TTL. This is
  the price of not having to chase expiry later.
- An untouched stream or NGO entry still expires independently of the instance
  and of other entries. Someone needs to call a keep-alive function for
  long-idle data, and monitoring should cover it.
- Because the instance TTL is shared, one active entry point keeps every
  instance key alive, but an idle contract can lose its whole instance at once.
- New storage keys must be assigned a storage type, a bump, and a place in
  [STORAGE.md](../STORAGE.md). Changing the shared constants changes fee and
  archival behavior for every contract that uses them.

## References

- `DAY_IN_LEDGERS`, `INSTANCE_BUMP_AMOUNT`, `INSTANCE_LIFETIME_THRESHOLD`,
  `PERSISTENT_BUMP_AMOUNT`, and `PERSISTENT_LIFETIME_THRESHOLD` in
  `contracts/common/src/lib.rs`
- `extend_instance_ttl`, `extend_stream_ttl`, and `extend_ngo_ttl` in each
  contract's `lib.rs`
- [STORAGE.md](../STORAGE.md), [MAINNET_CHECKLIST.md](../MAINNET_CHECKLIST.md)
