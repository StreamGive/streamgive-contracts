# 0003. The vault checks NGO verification itself, and only when money would move toward an NGO

- **Status:** Accepted
- **Date:** 2026-11-27 (recorded after the fact)
- **Applies to:** `donation-vault`, `ngo-registry`

## Context

Donors should only fund NGOs that have been vetted, and who counts as vetted is
decided by the registry's admin and approvers. The vault holds the money, so
something has to connect the two. The connection must also keep working when the
registry is slow to change: donors who have already committed funds should not
lose control of them, and an NGO's ability to collect what it has earned should
not depend on a second contract's state.

## Decision

The vault asks the registry directly, using an optional registry address.

- **The registry is optional and admin-configured.** `set_registry` stores a
  registry address and `clear_registry` removes it. With none configured, no
  check is performed.
- **The check fails closed.** `require_verified_ngo` calls the registry's
  `get_ngo` through `try_invoke_contract`. If the call fails for any reason, the
  NGO does not exist, or its record is not verified, the vault returns
  `Error::NgoNotVerified`. `get_ngo` reports an expired verification as not
  verified, so lapsed NGOs are rejected too.
- **The check runs when money would start or increase flowing to an NGO:**
  `create_stream`, `top_up`, `transfer_stream` (against the new NGO), a
  `modify_rate` that raises the rate, and `resume_stream`.
- **The check does not run on exits or reductions:** `withdraw`,
  `withdraw_batch`, `withdraw_all`, `cancel_stream`, and a rate decrease.
  Withdrawal checks only the vault's pause flag.

## Consequences

- **Revocation stops new money, not existing commitments.** A revoked NGO cannot
  receive new streams, top-ups, rate increases, or resumed streams, but streams
  that already exist keep accruing and can still be withdrawn from. Donors can
  cancel at any time and are never trapped. This residual risk is accepted as
  T-20 in [THREAT_MODEL.md](../THREAT_MODEL.md). The `pause` switch is the only
  emergency tool, and it stops withdrawals for everyone.
- **An NGO can always collect what it has earned,** independent of registry
  state.
- **The registry address is a trust point.** An admin who can call
  `set_registry` can point the vault at a contract that verifies anything. That
  is listed under admin key compromise (C-1) in the threat model, and the
  mainnet checklist covers registry configuration.
- **Each guarded call pays for a cross-contract invocation.** Calls that skip
  the check do not.
- The registry stays small and independent of the vault. It knows nothing about
  streams or money, and the vault needs only `get_ngo`.
- If the maintainers later want revocation to also freeze payouts, that is a
  change to `withdraw`, and this record should be superseded. The NGO-facing
  description in [NGOS.md](../NGOS.md) would need to change with it.

## Alternatives considered

- **Checking on every call, including withdrawals.** This would let revocation
  halt payouts immediately but would let a registry change override a donor's
  existing commitment and strand an NGO's accrued funds.
- **Having the registry call into the vault.** This would make the registry
  aware of streams and money and widen its attack surface.

## References

- `require_verified_ngo`, `set_registry`, `clear_registry`, `create_stream`,
  `top_up`, `transfer_stream`, `modify_rate`, `resume_stream`, and `withdraw`
  in `contracts/donation-vault/src/lib.rs`
- `get_ngo` and `verification_status` in `contracts/ngo-registry/src/lib.rs`
- [THREAT_MODEL.md](../THREAT_MODEL.md) (T-20, C-1),
  [ARCHITECTURE.md](../ARCHITECTURE.md), [ROLES.md](../ROLES.md)
- [NGOS.md](../NGOS.md), section "What happens if verification is revoked"
