use soroban_sdk::{token, Address, Env};

use crate::math;
use crate::types::{
    DataKey, Error, NgoRegistryClient, Stream, StreamStatus, INSTANCE_BUMP_AMOUNT,
    INSTANCE_LIFETIME_THRESHOLD, STREAM_BUMP_AMOUNT, STREAM_LIFETIME_THRESHOLD,
};

/// Keeps the contract instance (admin, config, next-id counter) from being
/// archived. Called on every state-changing entry point.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

/// Keeps a stream's persistent entry alive for 90 days past its last
/// touch, so a slow-draining stream doesn't get archived out from under
/// its donor and NGO between activity.
pub fn extend_stream_ttl(env: &Env, stream_id: u64) {
    env.storage().persistent().extend_ttl(
        &DataKey::Stream(stream_id),
        STREAM_LIFETIME_THRESHOLD,
        STREAM_BUMP_AMOUNT,
    );
}

/// Extends a cancelled stream beyond the normal retention period by the
/// configured indexing grace period.
pub fn extend_cancelled_stream_ttl(
    env: &Env,
    stream_id: u64,
    grace_ledgers: u32,
) -> Result<(), Error> {
    let bump_amount = STREAM_BUMP_AMOUNT
        .checked_add(grace_ledgers)
        .ok_or(Error::ArithmeticOverflow)?;
    env.storage().persistent().extend_ttl(
        &DataKey::Stream(stream_id),
        STREAM_LIFETIME_THRESHOLD,
        bump_amount,
    );
    Ok(())
}

pub fn admin_renounced(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::AdminRenounced)
        .unwrap_or(false)
}

/// Reads the configured admin and requires their auth, failing with
/// `Error::NotInitialized` if `init` hasn't been called yet. Shared by
/// every admin-gated entry point so the same three steps aren't repeated
/// at each call site.
pub fn require_admin(env: &Env) -> Result<Address, Error> {
    if admin_renounced(env) {
        return Err(Error::AdminRenounced);
    }
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)?;
    admin.require_auth();
    Ok(admin)
}

/// Returns `Err(Error::ContractPaused)` if an admin has paused the vault.
/// Checked at the top of every fund-moving entry point.
pub fn require_not_paused(env: &Env) -> Result<(), Error> {
    let paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false);
    if paused {
        return Err(Error::ContractPaused);
    }
    Ok(())
}

/// Reads the admin-configured upgrade timelock, in seconds. Defaults to zero
/// (timelock disabled) when an admin has not set one.
pub fn get_upgrade_delay(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::UpgradeDelay)
        .unwrap_or(0)
}

/// Reads the amount of `token` the vault holds on behalf of streams. Absent
/// means zero. See issue #346.
pub fn get_committed(env: &Env, token: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::CommittedBalance(token.clone()))
        .unwrap_or(0)
}

/// Writes the committed balance for `token`, removing the entry entirely when
/// it reaches zero so storage isn't left holding a stale key.
pub fn set_committed(env: &Env, token: &Address, amount: i128) {
    let key = DataKey::CommittedBalance(token.clone());
    if amount == 0 {
        env.storage().persistent().remove(&key);
        return;
    }
    env.storage().persistent().set(&key, &amount);
    env.storage()
        .persistent()
        .extend_ttl(&key, INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

/// Adjusts the committed balance for `token` by `delta`, which may be negative
/// (a payout or refund leaving the vault). Checked so a broken invariant aborts
/// rather than wrapping. See issue #346.
pub fn add_committed(env: &Env, token: &Address, delta: i128) -> Result<(), Error> {
    let next = get_committed(env, token)
        .checked_add(delta)
        .ok_or(Error::ArithmeticOverflow)?;
    set_committed(env, token, next);
    Ok(())
}

/// If an NGO registry is configured, verifies that `ngo` is approved/verified.
/// If no registry is configured, verification is skipped and this returns `Ok(())`.
/// Returns `Error::NgoNotVerified` if the NGO is unverified, unregistered, revoked,
/// or if the registry cross-contract call fails.
pub fn require_verified_ngo(env: &Env, ngo: &Address) -> Result<(), Error> {
    if let Some(registry_addr) = env
        .storage()
        .instance()
        .get::<_, Address>(&DataKey::Registry)
    {
        let registry = NgoRegistryClient::new(env, &registry_addr);
        let ngo_entry = registry
            .try_get_ngo(ngo)
            .map_err(|_| Error::NgoNotVerified)?
            .map_err(|_| Error::NgoNotVerified)?;
        if !ngo_entry.verified {
            return Err(Error::NgoNotVerified);
        }
    }
    Ok(())
}

/// Moves `amount` from a stream's `balance` into its `withdrawn` total,
/// failing with `Error::ArithmeticOverflow` rather than panicking if
/// either would leave i128's range.
pub fn record_payout(stream: &mut Stream, amount: i128) -> Result<(), Error> {
    stream.balance = stream
        .balance
        .checked_sub(amount)
        .ok_or(Error::ArithmeticOverflow)?;
    stream.withdrawn = stream
        .withdrawn
        .checked_add(amount)
        .ok_or(Error::ArithmeticOverflow)?;
    Ok(())
}

/// Derives the fee taken from a payout as `accrued - net`, where `net` is
/// what `pay_ngo` (or a batch's per-stream split) actually sent to the
/// NGO.
///
/// `net` never exceeds `accrued`, so the subtraction cannot fail while that
/// invariant holds. If it is ever broken this returns
/// `Error::ArithmeticOverflow` rather than reporting a fee of zero, so the
/// transaction aborts instead of publishing a misleading `withdraw` event.
pub fn fee_from_payout(accrued: i128, net: i128) -> Result<i128, Error> {
    accrued.checked_sub(net).ok_or(Error::ArithmeticOverflow)
}

/// Computes `amount * fee_bps / 10_000` without an intermediate overflow.
/// `amount * fee_bps` can exceed `i128::MAX` for a large `amount` even
/// though `fee_bps` is capped at `MAX_FEE_BPS` (1_000) — a naive
/// `amount * fee_bps` (or a `saturating_mul` that silently clamps the
/// overflowed product before dividing) both under-count the fee for such
/// amounts. Splitting `amount` into a quotient/remainder around the 10_000
/// divisor first keeps every multiplication in range: `quotient * fee_bps`
/// is bounded by `amount / 10_000`, and `remainder * fee_bps` is bounded by
/// `9_999 * 1_000`, both comfortably inside i128. The result is the exact
/// `floor(amount * fee_bps / 10_000)`, not an approximation of it.
pub fn compute_fee(env: &Env, token: &Address, amount: i128) -> i128 {
    let treasury: Option<Address> = env.storage().instance().get(&DataKey::Treasury);
    match treasury {
        Some(_) => {
            let fee_bps = effective_fee_bps(env, token) as i128;
            let quotient = amount / 10_000;
            let remainder = amount % 10_000;
            (quotient * fee_bps + (remainder * fee_bps) / 10_000).min(amount)
        }
        None => 0,
    }
}

/// Returns a token-specific fee override, or the global fee when no override
/// has been configured for `token`.
pub fn effective_fee_bps(env: &Env, token: &Address) -> u32 {
    env.storage()
        .persistent()
        .get(&DataKey::TokenFeeBps(token.clone()))
        .unwrap_or_else(|| env.storage().instance().get(&DataKey::FeeBps).unwrap_or(0))
}

/// Pays `amount` out to the NGO, skimming a protocol fee to the treasury
/// first if one is configured. With no treasury set, the full amount goes
/// to the NGO regardless of `fee_bps` — there's nowhere to send a fee.
///
/// Returns the net amount actually transferred to the NGO. This is the
/// single place the fee split is computed, so callers that report the
/// payout to their own callers (`withdraw`) can return exactly what the
/// NGO received rather than recomputing the fee and risking drift.
pub fn pay_ngo(env: &Env, token_client: &token::Client, ngo: &Address, amount: i128) -> i128 {
    if amount <= 0 {
        return 0;
    }

    let treasury: Option<Address> = env.storage().instance().get(&DataKey::Treasury);
    let fee = compute_fee(env, &token_client.address, amount);
    let net = amount - fee;

    if net > 0 {
        token_client.transfer(&env.current_contract_address(), ngo, &net);
    }
    if fee > 0 {
        if let Some(treasury_address) = treasury {
            token_client.transfer(&env.current_contract_address(), &treasury_address, &fee);
        }
    }

    net
}

/// Resolves where a payout for `ngo` should be sent: the payout address the
/// NGO configured via `set_payout_address`, or the NGO owner address itself
/// when none is set. Only `withdraw` and `withdraw_batch` route through this,
/// so an NGO can point withdrawals at a treasury wallet while its owner key
/// only authorises.
pub fn payout_destination(env: &Env, ngo: &Address) -> Address {
    env.storage()
        .persistent()
        .get(&DataKey::NgoPayoutAddress(ngo.clone()))
        .unwrap_or_else(|| ngo.clone())
}

/// Returns the admin-configured maximum per-second rate, if any. `None`
/// (the default) means rates are uncapped, so the check in `create_stream`
/// and `modify_rate` is skipped entirely.
pub fn max_rate_cap(env: &Env) -> Option<i128> {
    env.storage().instance().get(&DataKey::MaxRate)
}

/// Returns the admin-configured minimum time to depletion, in seconds. `0`
/// (the default) means no minimum is enforced, so the check in
/// `create_stream` and `modify_rate` is skipped entirely. See issue #340.
pub fn min_duration_seconds(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::MinDuration)
        .unwrap_or(0)
}

/// Returns true when a stream holding `balance` at a per-second `rate` would
/// deplete in fewer than `min_duration` seconds. A `min_duration` of `0`
/// disables the check.
///
/// The lifespan is measured with ceiling division so a stream that lasts a
/// full final partial second is not misjudged: a 100-unit balance at 30 per
/// second lasts 4 seconds, not 3. `ceil(balance / rate) < min_duration` is
/// rewritten as `balance <= (min_duration - 1) * rate`, which avoids the
/// division and reports the (unreachable) overflow case as "too short".
pub fn depletes_before_min_duration(balance: i128, rate: i128, min_duration: u64) -> bool {
    if min_duration == 0 {
        return false;
    }
    match ((min_duration - 1) as i128).checked_mul(rate) {
        Some(threshold) => balance <= threshold,
        None => true,
    }
}

pub fn record_outflow_totals(env: &Env, gross: i128, fee: i128) -> Result<(), Error> {
    if gross > 0 {
        let withdrawn: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalWithdrawn)
            .unwrap_or(0);
        let next = withdrawn
            .checked_add(gross)
            .ok_or(Error::ArithmeticOverflow)?;
        env.storage()
            .instance()
            .set(&DataKey::TotalWithdrawn, &next);
    }
    if fee > 0 {
        let collected: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalFeesCollected)
            .unwrap_or(0);
        let next = collected
            .checked_add(fee)
            .ok_or(Error::ArithmeticOverflow)?;
        env.storage()
            .instance()
            .set(&DataKey::TotalFeesCollected, &next);
    }
    Ok(())
}

/// The timestamp a stream's accrual is measured from: the later of its last
/// checkpoint and its `start`. Until `start` arrives this is `start` itself,
/// so `now - base` is zero and nothing accrues — the cliff — even though
/// `last_update` may have been checkpointed earlier. See issue #337.
pub fn accrual_base(stream: &Stream) -> u64 {
    stream.last_update.max(stream.start)
}

/// Settles the accrual accumulated since the stream's last checkpoint.
///
/// This is shared by every operation that changes a stream's balance or rate
/// so payout accounting, checked arithmetic, and the checkpoint timestamp
/// cannot drift between entry points.
///
/// It marks an emptied Active stream `Drained` but does **not** emit the
/// `drained` event itself: `cancel_stream` and `top_up` reuse this to settle a
/// stream they are about to reclassify (Cancelled / Active), so the "ran to
/// completion" event would be spurious for them. Every other caller goes
/// through the [`settle`] wrapper, which announces the transition (issue #344).
pub fn settle_core(env: &Env, stream: &mut Stream, now: u64) -> Result<i128, Error> {
    let accrued = if stream.status == StreamStatus::Paused {
        0
    } else {
        let elapsed = now.saturating_sub(accrual_base(stream));
        math::accrued(stream.rate, elapsed, stream.balance)
    };

    if accrued > 0 {
        // Update stream state before the external token transfer
        record_payout(stream, accrued)?;
    }
    stream.last_update = now;

    // Mark as Drained if settle emptied the balance
    if stream.balance == 0 && stream.status == StreamStatus::Active {
        stream.status = StreamStatus::Drained;
    }

    // External token transfer is the last step
    if accrued > 0 {
        // The accrued amount leaves the vault's committed balance here: this
        // is the single place a payout is sent, so every entry point that
        // settles a stream inherits the reduction (issue #346).
        add_committed(env, &stream.token, -accrued)?;
        let token_client = token::Client::new(env, &stream.token);
        let fee = compute_fee(env, &stream.token, accrued);
        record_outflow_totals(env, accrued, fee)?;
        pay_ngo(env, &token_client, &stream.ngo, accrued);
    }

    Ok(accrued)
}

/// Settles a stream and, if that emptied it, publishes the `drained` event
/// (issue #344). Wraps [`settle_core`] so every settle-driven drain announces
/// the transition exactly once, from the same place the status flips.
pub fn settle(env: &Env, stream_id: u64, stream: &mut Stream, now: u64) -> Result<i128, Error> {
    use crate::events::DrainedEvent;
    let was_active = stream.status == StreamStatus::Active;
    let accrued = settle_core(env, stream, now)?;
    if was_active && stream.status == StreamStatus::Drained {
        DrainedEvent {
            stream_id,
            total_paid: stream.withdrawn,
        }
        .publish(env);
    }
    Ok(accrued)
}
