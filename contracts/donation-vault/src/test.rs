#![cfg(test)]

use super::*;
use soroban_sdk::testutils::storage::{Instance as _, Persistent as _};
use soroban_sdk::testutils::{
    Address as _, AuthorizedFunction, Events as _, Ledger, MockAuth, MockAuthInvoke,
};
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::xdr::{ContractEventBody, ScSymbol, ScVal, ScVec};
use soroban_sdk::{IntoVal, Symbol, TryFromVal, Val, Vec};

/// The topics and data of the most recently published event, regardless of
/// which contract emitted it — vault entry points always publish their own
/// event last, after any token transfer, so this is the vault's event.
///
/// The SDK stopped implementing `PartialEq` on raw `Val`s, so this keeps the
/// event in XDR form and, when compared against an expected
/// `(topics, data)` pair, converts that pair to XDR too. That lets the
/// existing `assert_eq!(last_event(..), (..).into_val(..))` assertions stay
/// as they are.
struct Event {
    env: Env,
    topics: ScVec,
    data: ScVal,
}

impl core::fmt::Debug for Event {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Event")
            .field("topics", &self.topics)
            .field("data", &self.data)
            .finish()
    }
}

impl PartialEq<(Vec<Val>, Val)> for Event {
    fn eq(&self, other: &(Vec<Val>, Val)) -> bool {
        let topics: ScVec = (&other.0).into();
        let data = ScVal::try_from_val(&self.env, &other.1).unwrap();
        self.topics == topics && self.data == data
    }
}

fn last_event(env: &Env) -> Event {
    let events = env.events().all();
    let event = events.events().last().unwrap();
    let ContractEventBody::V0(body) = &event.body;
    Event {
        env: env.clone(),
        topics: body.topics.clone().into(),
        data: body.data.clone(),
    }
}

fn create_token<'a>(env: &Env, admin: &Address) -> (TokenClient<'a>, StellarAssetClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    (
        TokenClient::new(env, &sac.address()),
        StellarAssetClient::new(env, &sac.address()),
    )
}

struct Setup<'a> {
    env: Env,
    client: DonationVaultClient<'a>,
    token: TokenClient<'a>,
    token_admin: StellarAssetClient<'a>,
    donor: Address,
    ngo: Address,
}

fn setup() -> Setup<'static> {
    let env = Env::default();
    env.mock_all_auths();

    // New entries (the token's included) start with at least 20 days of
    // TTL, so the TTL tests can age the ledger past the vault's bump
    // thresholds without archiving the token out from under a transfer.
    // 20 days is still below both thresholds, so the vault's own bumps on
    // init and create_stream are what set its entries' TTLs.
    env.ledger().with_mut(|l| {
        l.min_persistent_entry_ttl = 20 * DAY_IN_LEDGERS;
        l.max_entry_ttl = 365 * DAY_IN_LEDGERS;
    });

    let contract_id = env.register(DonationVault, ());
    let client = DonationVaultClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    let token_issuer = Address::generate(&env);
    let (token, token_admin) = create_token(&env, &token_issuer);

    let donor = Address::generate(&env);
    let ngo = Address::generate(&env);

    Setup {
        env,
        client,
        token,
        token_admin,
        donor,
        ngo,
    }
}

#[test]
fn full_lifecycle_create_accrue_withdraw_cancel() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    // Capture the event before any other contract call: the SDK only exposes
    // the events of the most recent invocation.
    let created = last_event(&s.env);

    assert_eq!(s.token.balance(&s.donor), 0);
    assert_eq!(s.token.balance(&s.client.address), 1_000);
    assert_eq!(
        created,
        (
            (symbol_short!("created"), stream_id).into_val(&s.env),
            (
                s.donor.clone(),
                s.ngo.clone(),
                s.token.address.clone(),
                1_000i128,
                10i128
            )
                .into_val(&s.env),
        )
    );

    // 50 seconds pass -> 10/s * 50 = 500 should be withdrawable.
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let withdrawn = s.client.withdraw(&stream_id);
    let withdraw = last_event(&s.env);
    assert_eq!(withdrawn, 500);
    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(
        withdraw,
        (
            (symbol_short!("withdraw"), stream_id).into_val(&s.env),
            500i128.into_val(&s.env),
        )
    );

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 500);
    assert_eq!(stream.withdrawn, 500);

    // 20 more seconds pass, then the donor cancels.
    s.env.ledger().with_mut(|l| l.timestamp += 20);
    s.client.cancel_stream(&stream_id);
    let cancel = last_event(&s.env);

    // 200 more settles to the NGO on cancel; the untouched 300 refunds to the donor.
    assert_eq!(s.token.balance(&s.ngo), 700);
    assert_eq!(s.token.balance(&s.donor), 300);
    assert_eq!(
        cancel,
        (
            (symbol_short!("cancel"), stream_id).into_val(&s.env),
            (200i128, 300i128).into_val(&s.env),
        )
    );

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.rate, 0);
}

#[test]
fn top_up_and_modify_rate_settle_before_changing() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 10); // 100 accrues

    s.client.top_up(&stream_id, &500);
    let topup = last_event(&s.env);

    assert_eq!(s.token.balance(&s.ngo), 100);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_400); // 1000 - 100 accrued + 500 top-up
    assert_eq!(stream.rate, 10);
    assert_eq!(
        topup,
        (
            (symbol_short!("topup"), stream_id).into_val(&s.env),
            500i128.into_val(&s.env),
        )
    );

    s.env.ledger().with_mut(|l| l.timestamp += 5); // 50 more accrues at the old rate

    s.client.modify_rate(&stream_id, &20);
    let ratemod = last_event(&s.env);

    assert_eq!(s.token.balance(&s.ngo), 150);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.rate, 20);
    assert_eq!(stream.balance, 1_350); // 1400 - 50
    assert_eq!(
        ratemod,
        (
            (symbol_short!("ratemod"), stream_id).into_val(&s.env),
            20i128.into_val(&s.env),
        )
    );
}

#[test]
fn created_at_is_set_once_and_never_changes() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let stream = s.client.get_stream(&stream_id);
    let created_at = stream.created_at;
    assert_eq!(created_at, stream.last_update);

    // Withdraw, top-up, and modify_rate all move last_update forward, but
    // none of them should touch created_at.
    s.env.ledger().with_mut(|l| l.timestamp += 50);
    s.client.withdraw(&stream_id);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.created_at, created_at);
    assert_ne!(stream.last_update, created_at);
}

#[test]
fn create_stream_rejects_non_positive_amounts() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &0, &10);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &0);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn propose_then_accept_admin_transfers_control() {
    let s = setup();
    let old_admin = s.client.admin();
    let new_admin = Address::generate(&s.env);

    s.client.propose_admin(&new_admin);
    let proposed = last_event(&s.env);
    // Admin hasn't changed yet — only proposed.
    assert_eq!(s.client.admin(), old_admin);
    assert_eq!(
        proposed,
        (
            (symbol_short!("propadmin"),).into_val(&s.env),
            new_admin.into_val(&s.env),
        )
    );

    s.client.accept_admin();
    let accepted = last_event(&s.env);
    assert_eq!(s.client.admin(), new_admin);
    assert_eq!(
        accepted,
        (
            (symbol_short!("acptadmin"),).into_val(&s.env),
            new_admin.into_val(&s.env),
        )
    );

    // The new admin can act as admin.
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    assert_eq!(s.client.treasury(), Some(treasury));
}

#[test]
fn accept_admin_without_proposal_fails() {
    let s = setup();
    let result = s.client.try_accept_admin();
    assert_eq!(result, Err(Ok(Error::NoPendingAdmin)));
}

#[test]
#[should_panic]
fn old_admin_loses_admin_gated_access_after_transfer() {
    let s = setup();
    let old_admin = s.client.admin();
    let new_admin = Address::generate(&s.env);

    s.client.propose_admin(&new_admin);
    s.client.accept_admin();

    // set_treasury requires the current admin's auth; only the old admin
    // authorizes this call, and the old admin is no longer admin.
    let treasury = Address::generate(&s.env);
    s.env.mock_auths(&[MockAuth {
        address: &old_admin,
        invoke: &MockAuthInvoke {
            contract: &s.client.address,
            fn_name: "set_treasury",
            args: (treasury.clone(),).into_val(&s.env),
            sub_invokes: &[],
        },
    }]);
    s.client.set_treasury(&treasury);
}

#[test]
fn pending_accrual_matches_withdraw_without_mutating_state() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // 50 seconds pass -> 10/s * 50 = 500 should be pending.
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let pending = s.client.pending_accrual(&stream_id);
    assert_eq!(pending, 500);

    // Checking pending_accrual must not move funds or touch the stream.
    assert_eq!(s.token.balance(&s.ngo), 0);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);

    // It should match exactly what withdraw actually pays out.
    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, pending);
}

#[test]
fn withdraw_with_nothing_accrued_fails() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let result = s.client.try_withdraw(&stream_id);
    assert_eq!(result, Err(Ok(Error::NothingToWithdraw)));
}

#[test]
fn pause_blocks_create_but_not_cancel() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.pause();
    let paused = last_event(&s.env);
    assert!(s.client.paused());
    assert_eq!(
        paused,
        (
            (symbol_short!("pause"),).into_val(&s.env),
            ().into_val(&s.env),
        )
    );

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &100, &10);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));

    // Cancelling still works while paused, so donors are never trapped.
    s.client.cancel_stream(&stream_id);
    assert_eq!(s.token.balance(&s.donor), 1_000);
}

#[test]
fn unpause_restores_normal_operation() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    s.client.pause();
    s.client.unpause();
    let unpaused = last_event(&s.env);
    assert!(!s.client.paused());
    assert_eq!(
        unpaused,
        (
            (symbol_short!("unpause"),).into_val(&s.env),
            ().into_val(&s.env),
        )
    );

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
}

#[test]
fn withdraw_with_no_treasury_takes_no_fee() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.client.set_fee_bps(&500); // configured, but no treasury yet

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 500);
    assert_eq!(s.token.balance(&s.ngo), 500);
}

#[test]
fn withdraw_splits_protocol_fee_to_treasury() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5%

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues

    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 500);
    assert_eq!(s.token.balance(&treasury), 25);
    assert_eq!(s.token.balance(&s.ngo), 475);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.withdrawn, 500); // bookkeeping tracks the gross amount
}

#[test]
fn cancel_stream_splits_protocol_fee_on_accrued_but_not_on_refund() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5%

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 20); // 200 accrues

    s.client.cancel_stream(&stream_id);

    // 5% of the 200 accrued goes to the treasury; the rest settles to the NGO.
    assert_eq!(s.token.balance(&treasury), 10);
    assert_eq!(s.token.balance(&s.ngo), 190);
    // The untouched 800 refunds to the donor in full — no fee on refunds.
    assert_eq!(s.token.balance(&s.donor), 800);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.withdrawn, 200); // bookkeeping tracks the gross accrued amount
}

#[test]
fn small_payout_rounds_protocol_fee_down_to_zero() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5%

    // 19 units at 5% is 0.95, and the fee is computed with integer
    // division, so it truncates to nothing.
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &19);
    s.env.ledger().with_mut(|l| l.timestamp += 1); // 19 accrues

    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 19);

    // The rounding favours the NGO: it keeps the whole payout rather than
    // the treasury rounding its cut up to 1.
    assert_eq!(s.token.balance(&s.ngo), 19);
    assert_eq!(s.token.balance(&treasury), 0);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.withdrawn, 19);
}

#[test]
fn protocol_fee_becomes_nonzero_at_the_rounding_boundary() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5%

    // 20 is the smallest payout at 5% that leaves a whole unit of fee, so
    // it pins the other side of the boundary that 19 truncates below.
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &20);
    s.env.ledger().with_mut(|l| l.timestamp += 1); // 20 accrues

    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 20);
    assert_eq!(s.token.balance(&treasury), 1);
    assert_eq!(s.token.balance(&s.ngo), 19);
}

#[test]
fn set_fee_bps_rejects_over_cap() {
    let s = setup();
    let result = s.client.try_set_fee_bps(&1_001);
    assert_eq!(result, Err(Ok(Error::FeeTooHigh)));
}

#[test]
#[should_panic]
fn withdraw_fails_for_non_ngo_caller() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 50);

    // Only the donor authorizes this call; withdraw requires the ngo's auth,
    // so it must fail even though the donor is a party to the stream.
    s.env.mock_auths(&[MockAuth {
        address: &s.donor,
        invoke: &MockAuthInvoke {
            contract: &s.client.address,
            fn_name: "withdraw",
            args: (stream_id,).into_val(&s.env),
            sub_invokes: &[],
        },
    }]);

    s.client.withdraw(&stream_id);
}

#[test]
fn withdraw_and_cancel_on_fully_drained_stream_are_no_ops() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // 100 seconds at 10/s would accrue 1_000, exactly draining the balance.
    s.env.ledger().with_mut(|l| l.timestamp += 100);

    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 1_000);
    assert_eq!(s.token.balance(&s.ngo), 1_000);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.withdrawn, 1_000);

    // More time passes, but there's nothing left to accrue.
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let result = s.client.try_withdraw(&stream_id);
    assert_eq!(result, Err(Ok(Error::NothingToWithdraw)));

    // Cancelling a drained stream settles and refunds nothing.
    s.client.cancel_stream(&stream_id);
    assert_eq!(s.token.balance(&s.ngo), 1_000);
    assert_eq!(s.token.balance(&s.donor), 0);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.rate, 0);
    assert_eq!(stream.withdrawn, 1_000);
}

/// Counts the `transfer` events the given token has emitted so far, so a test
/// can snapshot the count before a call and check how many transfers it made.
fn token_transfers(env: &Env, token: &Address) -> u32 {
    let transfer = ScSymbol::try_from("transfer").unwrap();
    env.events()
        .all()
        .filter_by_contract(token)
        .events()
        .iter()
        .filter(|event| match &event.body {
            ContractEventBody::V0(body) => body
                .topics
                .first()
                .map(|topic| matches!(topic, ScVal::Symbol(sym) if sym == &transfer))
                .unwrap_or(false),
        })
        .count() as u32
}

fn stream_ids(env: &Env, ids: &[u64]) -> Vec<u64> {
    let mut v = Vec::new(env);
    for id in ids {
        v.push_back(*id);
    }
    v
}

#[test]
fn withdraw_batch_pays_mixed_tokens_with_one_transfer_each() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let (token_b, token_b_admin) = create_token(&s.env, &Address::generate(&s.env));
    token_b_admin.mint(&s.donor, &2_000);

    // Two streams on token A at different rates, one on token B.
    let a0 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let a1 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &20);
    let b0 = s
        .client
        .create_stream(&s.donor, &s.ngo, &token_b.address, &1_000, &5);

    s.env.ledger().with_mut(|l| l.timestamp += 10); // a0: 100, a1: 200, b0: 50

    let ids = stream_ids(&s.env, &[a0, a1, b0]);
    let withdrawn = s.client.withdraw_batch(&ids);
    // Read the batch invocation's transfers before any later contract call
    // clears the event buffer.
    let transfers_a = token_transfers(&s.env, &s.token.address);
    let transfers_b = token_transfers(&s.env, &token_b.address);

    // Amounts come back in input order.
    assert_eq!(withdrawn.get(0), Some(100));
    assert_eq!(withdrawn.get(1), Some(200));
    assert_eq!(withdrawn.get(2), Some(50));

    assert_eq!(s.token.balance(&s.ngo), 300);
    assert_eq!(token_b.balance(&s.ngo), 50);

    // The two token-A streams are paid in a single transfer; token B gets
    // its own.
    assert_eq!(transfers_a, 1);
    assert_eq!(transfers_b, 1);

    // Every stream was checkpointed and debited by its own accrual.
    let a0_stream = s.client.get_stream(&a0);
    assert_eq!(a0_stream.balance, 900);
    assert_eq!(a0_stream.withdrawn, 100);
    assert_eq!(a0_stream.last_update, 10);

    let a1_stream = s.client.get_stream(&a1);
    assert_eq!(a1_stream.balance, 800);
    assert_eq!(a1_stream.withdrawn, 200);

    let b0_stream = s.client.get_stream(&b0);
    assert_eq!(b0_stream.balance, 950);
    assert_eq!(b0_stream.withdrawn, 50);
}

#[test]
fn withdraw_batch_skips_streams_with_nothing_accrued() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let older = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    // Created after the first checkpoint, so it has accrued nothing yet.
    let fresh = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let ids = stream_ids(&s.env, &[older, fresh]);
    let withdrawn = s.client.withdraw_batch(&ids);

    assert_eq!(withdrawn.get(0), Some(100));
    assert_eq!(withdrawn.get(1), Some(0));

    // Only the stream that had something to withdraw paid out.
    assert_eq!(s.token.balance(&s.ngo), 100);

    // The skipped stream was left completely untouched.
    let untouched = s.client.get_stream(&fresh);
    assert_eq!(untouched.balance, 1_000);
    assert_eq!(untouched.withdrawn, 0);
    assert_eq!(untouched.last_update, 10);
}

#[test]
fn withdraw_batch_unknown_stream_aborts_the_whole_batch() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues

    let ids = stream_ids(&s.env, &[stream_id, 999]);
    let result = s.client.try_withdraw_batch(&ids);
    assert_eq!(result, Err(Ok(Error::StreamNotFound)));

    // The whole batch rolled back: nothing was paid and the valid stream
    // wasn't checkpointed or debited.
    assert_eq!(s.token.balance(&s.ngo), 0);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);
    assert_eq!(stream.last_update, 0);
}

#[test]
fn withdraw_batch_rejects_streams_owned_by_different_ngos() {
    let s = setup();
    let other_ngo = Address::generate(&s.env);
    s.token_admin.mint(&s.donor, &2_000);

    let mine = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let theirs = s
        .client
        .create_stream(&s.donor, &other_ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let ids = stream_ids(&s.env, &[mine, theirs]);
    let result = s.client.try_withdraw_batch(&ids);
    assert_eq!(result, Err(Ok(Error::MixedNgo)));

    assert_eq!(s.token.balance(&s.ngo), 0);
    assert_eq!(s.token.balance(&other_ngo), 0);
    assert_eq!(s.client.get_stream(&mine).balance, 1_000);
}

#[test]
fn withdraw_batch_skims_fee_once_from_the_aggregated_token_payout() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5%

    // 19 units each: 5% truncates to 0 per stream but rounds to 1 whole
    // unit across the aggregated 38, so this pins that the fee is computed
    // once on the token sum rather than once per stream.
    let a = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &19);
    let b = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &19);
    s.env.ledger().with_mut(|l| l.timestamp += 1);

    let ids = stream_ids(&s.env, &[a, b]);
    let withdrawn = s.client.withdraw_batch(&ids);
    assert_eq!(withdrawn.get(0), Some(19));
    assert_eq!(withdrawn.get(1), Some(19));

    assert_eq!(s.token.balance(&treasury), 1);
    assert_eq!(s.token.balance(&s.ngo), 37);
}

#[test]
fn withdraw_batch_with_no_streams_is_a_noop() {
    let s = setup();
    let ids: Vec<u64> = Vec::new(&s.env);

    let withdrawn = s.client.withdraw_batch(&ids);

    assert_eq!(withdrawn.len(), 0);
    assert!(s.env.auths().is_empty());
}

#[test]
fn withdraw_batch_requires_ngo_auth() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);
    let a = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let b = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let ids = stream_ids(&s.env, &[a, b]);
    s.client.withdraw_batch(&ids);

    // Both streams share one NGO, so the batch needs a single auth.
    assert_auth_required_from(&s, &s.ngo, "withdraw_batch");
}

#[test]
fn withdraw_batch_bumps_instance_and_stream_ttls() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);
    let a = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let b = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    age_past_thresholds(&s, Some(a));
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let ids = stream_ids(&s.env, &[a, b]);
    s.client.withdraw_batch(&ids);

    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);
    assert_eq!(stream_ttl(&s, a), STREAM_BUMP_AMOUNT);
    assert_eq!(stream_ttl(&s, b), STREAM_BUMP_AMOUNT);
}

#[test]
fn cancel_stream_twice_is_harmless() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues

    s.client.cancel_stream(&stream_id);
    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.token.balance(&s.donor), 500);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.rate, 0);
    assert_eq!(stream.withdrawn, 500);

    // Cancelling again settles zero (rate and balance are already zero) and
    // refunds zero, leaving balances and stream state unchanged.
    s.env.ledger().with_mut(|l| l.timestamp += 50);
    s.client.cancel_stream(&stream_id);

    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.token.balance(&s.donor), 500);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.rate, 0);
    assert_eq!(stream.withdrawn, 500);
}

#[test]
fn modify_rate_rejects_non_positive_rate() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let result = s.client.try_modify_rate(&stream_id, &0);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));

    let result = s.client.try_modify_rate(&stream_id, &-1);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));

    // A rejected call settles nothing and changes nothing: pausing a
    // stream goes through cancel_stream, not a zero rate.
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.rate, 10);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);
}

#[test]
fn modify_rate_on_unknown_stream_fails() {
    let s = setup();

    let result = s.client.try_modify_rate(&999, &10);
    assert_eq!(result, Err(Ok(Error::StreamNotFound)));
}

#[test]
fn modify_rate_checks_the_rate_before_the_stream_id() {
    let s = setup();

    // Both arguments are bad. The rate is validated before the stream is
    // looked up, so the caller gets InvalidAmount rather than
    // StreamNotFound — worth pinning so the order can't quietly flip.
    let result = s.client.try_modify_rate(&999, &0);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn top_up_rejects_non_positive_amount() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_500);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let result = s.client.try_top_up(&stream_id, &0);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));

    let result = s.client.try_top_up(&stream_id, &-100);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));

    // top_up settles accrued funds and pulls tokens from the donor, so a
    // rejected call has to leave both the stream and the balances alone.
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);
    assert_eq!(s.token.balance(&s.donor), 500);
    assert_eq!(s.token.balance(&s.ngo), 0);
}

#[test]
fn top_up_on_unknown_stream_fails() {
    let s = setup();

    let result = s.client.try_top_up(&999, &100);
    assert_eq!(result, Err(Ok(Error::StreamNotFound)));
}

#[test]
fn top_up_checks_the_amount_before_the_stream_id() {
    let s = setup();

    // Both arguments are bad. The amount is validated before the stream is
    // looked up, so the caller gets InvalidAmount rather than
    // StreamNotFound — worth pinning so the order can't quietly flip.
    let result = s.client.try_top_up(&999, &0);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn get_stream_on_unknown_id_fails() {
    let s = setup();

    // Debug on Stream is what lets assert_eq! take the whole
    // Result<Result<Stream, _>, _> here instead of matching on it.
    let result = s.client.try_get_stream(&999);
    assert_eq!(result, Err(Ok(Error::StreamNotFound)));
}

#[test]
fn create_stream_stores_every_field() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.env.ledger().with_mut(|l| l.timestamp = 12_345);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // PartialEq on Stream lets one assertion cover the whole struct, so a
    // newly added field can't slip in unchecked the way it would with a
    // handful of per-field assertions.
    assert_eq!(
        s.client.get_stream(&stream_id),
        Stream {
            donor: s.donor.clone(),
            ngo: s.ngo.clone(),
            token: s.token.address.clone(),
            rate: 10,
            balance: 1_000,
            withdrawn: 0,
            created_at: 12_345,
            last_update: 12_345,
        }
    );
}

fn instance_ttl(s: &Setup) -> u32 {
    s.env
        .as_contract(&s.client.address, || s.env.storage().instance().get_ttl())
}

fn stream_ttl(s: &Setup, stream_id: u64) -> u32 {
    s.env.as_contract(&s.client.address, || {
        s.env
            .storage()
            .persistent()
            .get_ttl(&DataKey::Stream(stream_id))
    })
}

/// Moves the ledger forward two days, which drops both the instance and a
/// freshly bumped stream below their lifetime thresholds. Asserts that it
/// did, so a test calling this can't pass just because nothing needed a bump.
fn age_past_thresholds(s: &Setup, stream_id: Option<u64>) {
    s.env
        .ledger()
        .with_mut(|l| l.sequence_number += 2 * DAY_IN_LEDGERS);
    assert!(instance_ttl(s) < INSTANCE_LIFETIME_THRESHOLD);
    if let Some(stream_id) = stream_id {
        assert!(stream_ttl(s, stream_id) < STREAM_LIFETIME_THRESHOLD);
    }
}

fn create_ttl_test_stream(s: &Setup) -> u64 {
    s.token_admin.mint(&s.donor, &2_000);
    s.client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10)
}

#[test]
fn create_stream_bumps_instance_and_stream_ttl() {
    let s = setup();
    let stream_id = create_ttl_test_stream(&s);

    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);
    assert_eq!(stream_ttl(&s, stream_id), STREAM_BUMP_AMOUNT);
}

#[test]
fn withdraw_bumps_instance_and_stream_ttl() {
    let s = setup();
    let stream_id = create_ttl_test_stream(&s);
    age_past_thresholds(&s, Some(stream_id));
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    s.client.withdraw(&stream_id);

    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);
    assert_eq!(stream_ttl(&s, stream_id), STREAM_BUMP_AMOUNT);
}

#[test]
fn cancel_stream_bumps_instance_and_stream_ttl() {
    let s = setup();
    let stream_id = create_ttl_test_stream(&s);
    age_past_thresholds(&s, Some(stream_id));

    s.client.cancel_stream(&stream_id);

    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);
    assert_eq!(stream_ttl(&s, stream_id), STREAM_BUMP_AMOUNT);
}

#[test]
fn top_up_bumps_instance_and_stream_ttl() {
    let s = setup();
    let stream_id = create_ttl_test_stream(&s);
    age_past_thresholds(&s, Some(stream_id));

    s.client.top_up(&stream_id, &500);

    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);
    assert_eq!(stream_ttl(&s, stream_id), STREAM_BUMP_AMOUNT);
}

#[test]
fn modify_rate_bumps_instance_and_stream_ttl() {
    let s = setup();
    let stream_id = create_ttl_test_stream(&s);
    age_past_thresholds(&s, Some(stream_id));

    s.client.modify_rate(&stream_id, &20);

    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);
    assert_eq!(stream_ttl(&s, stream_id), STREAM_BUMP_AMOUNT);
}

#[test]
fn extend_stream_bumps_stream_ttl() {
    let s = setup();
    let stream_id = create_ttl_test_stream(&s);
    age_past_thresholds(&s, Some(stream_id));

    s.client.extend_stream(&stream_id);

    assert_eq!(stream_ttl(&s, stream_id), STREAM_BUMP_AMOUNT);
}

#[test]
fn admin_writes_bump_instance_ttl() {
    let s = setup();
    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT); // from init

    let new_admin = Address::generate(&s.env);
    let treasury = Address::generate(&s.env);

    age_past_thresholds(&s, None);
    s.client.pause();
    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);

    age_past_thresholds(&s, None);
    s.client.unpause();
    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);

    age_past_thresholds(&s, None);
    s.client.set_treasury(&treasury);
    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);

    age_past_thresholds(&s, None);
    s.client.set_fee_bps(&100);
    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);

    age_past_thresholds(&s, None);
    s.client.propose_admin(&new_admin);
    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);

    age_past_thresholds(&s, None);
    s.client.cancel_admin_proposal();
    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);

    s.client.propose_admin(&new_admin);
    age_past_thresholds(&s, None);
    s.client.accept_admin();
    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);
}

/// Rewrites a stored stream in place, for pushing its bookkeeping to the
/// edge of i128 — no real token supply could get it there.
fn overwrite_stream(s: &Setup, stream_id: u64, edit: impl FnOnce(&mut Stream)) {
    s.env.as_contract(&s.client.address, || {
        let key = DataKey::Stream(stream_id);
        let mut stream: Stream = s.env.storage().persistent().get(&key).unwrap();
        edit(&mut stream);
        s.env.storage().persistent().set(&key, &stream);
    });
}

#[test]
fn top_up_past_i128_max_balance_returns_overflow_error() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_001);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    overwrite_stream(&s, stream_id, |stream| stream.balance = i128::MAX);

    let result = s.client.try_top_up(&stream_id, &1);
    assert_eq!(result, Err(Ok(Error::ArithmeticOverflow)));

    // The failed call rolls back, so the donor keeps the unit it tried to add.
    assert_eq!(s.token.balance(&s.donor), 1);
}

#[test]
fn withdraw_past_i128_max_withdrawn_returns_overflow_error() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    overwrite_stream(&s, stream_id, |stream| stream.withdrawn = i128::MAX);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let result = s.client.try_withdraw(&stream_id);
    assert_eq!(result, Err(Ok(Error::ArithmeticOverflow)));
    assert_eq!(s.token.balance(&s.ngo), 0);
}

#[test]
fn cancel_stream_past_i128_max_withdrawn_returns_overflow_error() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    overwrite_stream(&s, stream_id, |stream| stream.withdrawn = i128::MAX);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let result = s.client.try_cancel_stream(&stream_id);
    assert_eq!(result, Err(Ok(Error::ArithmeticOverflow)));
    assert_eq!(s.token.balance(&s.ngo), 0);
    assert_eq!(s.token.balance(&s.donor), 0);
}

#[test]
fn modify_rate_past_i128_max_withdrawn_returns_overflow_error() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    overwrite_stream(&s, stream_id, |stream| stream.withdrawn = i128::MAX);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let result = s.client.try_modify_rate(&stream_id, &20);
    assert_eq!(result, Err(Ok(Error::ArithmeticOverflow)));
    assert_eq!(s.client.get_stream(&stream_id).rate, 10);
}

/// Asserts that the last top-level call required auth from exactly one
/// address, `expected`, and that it was for `fn_name` on the vault.
/// mock_all_auths() lets any require_auth pass, so without this a
/// require_auth removed or moved to the wrong address would go unnoticed.
fn assert_auth_required_from(s: &Setup, expected: &Address, fn_name: &str) {
    let auths = s.env.auths();
    assert_eq!(auths.len(), 1, "expected exactly one authorizer");

    let (address, invocation) = &auths[0];
    assert_eq!(address, expected);
    match &invocation.function {
        AuthorizedFunction::Contract((contract, function, _)) => {
            assert_eq!(contract, &s.client.address);
            assert_eq!(function, &Symbol::new(&s.env, fn_name));
        }
        _ => panic!("expected a contract invocation"),
    }
}

#[test]
fn create_stream_requires_donor_auth() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    s.client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    assert_auth_required_from(&s, &s.donor, "create_stream");
}

#[test]
fn withdraw_requires_ngo_auth() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    s.client.withdraw(&stream_id);

    assert_auth_required_from(&s, &s.ngo, "withdraw");
}

#[test]
fn cancel_stream_requires_donor_auth() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.cancel_stream(&stream_id);

    assert_auth_required_from(&s, &s.donor, "cancel_stream");
}

#[test]
fn top_up_requires_donor_auth() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_500);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.top_up(&stream_id, &500);

    assert_auth_required_from(&s, &s.donor, "top_up");
}

#[test]
fn modify_rate_requires_donor_auth() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.modify_rate(&stream_id, &20);

    assert_auth_required_from(&s, &s.donor, "modify_rate");
}

#[test]
fn extend_stream_requires_no_auth() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.extend_stream(&stream_id);

    assert!(s.env.auths().is_empty());
}

#[test]
fn admin_entry_points_require_admin_auth() {
    let s = setup();
    let admin = s.client.admin();

    s.client.pause();
    assert_auth_required_from(&s, &admin, "pause");

    s.client.unpause();
    assert_auth_required_from(&s, &admin, "unpause");

    s.client.set_treasury(&Address::generate(&s.env));
    assert_auth_required_from(&s, &admin, "set_treasury");

    s.client.set_fee_bps(&100);
    assert_auth_required_from(&s, &admin, "set_fee_bps");

    let new_admin = Address::generate(&s.env);
    s.client.propose_admin(&new_admin);
    assert_auth_required_from(&s, &admin, "propose_admin");

    s.client.cancel_admin_proposal();
    assert_auth_required_from(&s, &admin, "cancel_admin_proposal");
}

#[test]
fn accept_admin_requires_pending_admin_auth() {
    let s = setup();
    let new_admin = Address::generate(&s.env);
    s.client.propose_admin(&new_admin);

    s.client.accept_admin();

    // The proposed address, not the outgoing admin, has to accept.
    assert_auth_required_from(&s, &new_admin, "accept_admin");
}
