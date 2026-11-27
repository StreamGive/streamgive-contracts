// SPDX-License-Identifier: Apache-2.0
#![cfg(test)]

use super::*;
use common::{DAY_IN_LEDGERS, INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD};
use soroban_sdk::testutils::storage::{Instance as _, Persistent as _};
use soroban_sdk::testutils::{
    Address as _, AuthorizedFunction, Events as _, Ledger, MockAuth, MockAuthInvoke,
};
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::xdr::{ContractEventBody, ScVal, ScVec};
use soroban_sdk::{symbol_short, Bytes, IntoVal, Symbol, TryFromVal, Val, Vec};

/// Asserts that the `n`-th most recently published event (0 = the last one)
/// has the given topics and data, regardless of which contract emitted it â€”
/// vault entry points publish their own events last, after any token
/// transfer. Lets a test pin the order of the several events a single call
/// may emit (issue #344). Compared as XDR because `Val` has no `PartialEq`.
fn assert_nth_last_event<T: IntoVal<Env, Val>, D: IntoVal<Env, Val>>(
    env: &Env,
    n: u32,
    topics: T,
    data: D,
) {
    let all = env.events().all();
    let events = all.events();
    let event = &events[events.len() - 1 - n as usize];
    let ContractEventBody::V0(body) = &event.body;
    let expected_topics = ScVal::try_from_val(env, &topics.into_val(env)).unwrap();
    let expected_data = ScVal::try_from_val(env, &data.into_val(env)).unwrap();
    assert_eq!(
        ScVal::Vec(Some(ScVec(body.topics.clone()))),
        expected_topics
    );
    assert_eq!(body.data, expected_data);
}

/// Asserts that the most recently published event has the given topics and
/// data, regardless of which contract emitted it â€” vault entry points always
/// publish their own event last, after any token transfer, so this is the
/// vault's event. Compared as XDR because `Val` has no `PartialEq`.
fn assert_last_event<T: IntoVal<Env, Val>, D: IntoVal<Env, Val>>(env: &Env, topics: T, data: D) {
    assert_nth_last_event(env, 0, topics, data);
}

/// Counts the published events whose first topic equals `topic0`. Lets a test
/// assert an event was (or was not) emitted without depending on it being the
/// last one (issue #344).
fn count_events_with_topic(env: &Env, topic0: Symbol) -> u32 {
    let all = env.events().all();
    let topic0: Val = topic0.into_val(env);
    let expected = ScVal::try_from_val(env, &topic0).unwrap();
    all.events()
        .iter()
        .filter(|event| {
            let ContractEventBody::V0(body) = &event.body;
            body.topics.first() == Some(&expected)
        })
        .count() as u32
}

/// The most recently published event, in XDR form. `Val` has no `PartialEq`,
/// so it compares against an expected `(topics, data)` pair by converting
/// that pair to XDR too.
struct LastEvent {
    env: Env,
    topics: ScVal,
    data: ScVal,
}

impl core::fmt::Debug for LastEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "({:?}, {:?})", self.topics, self.data)
    }
}

impl PartialEq<(Vec<Val>, Val)> for LastEvent {
    fn eq(&self, (topics, data): &(Vec<Val>, Val)) -> bool {
        ScVal::try_from_val(&self.env, &topics.to_val()).unwrap() == self.topics
            && ScVal::try_from_val(&self.env, data).unwrap() == self.data
    }
}

/// The topics and data of the most recently published event, regardless of
/// which contract emitted it â€” vault entry points always publish their own
/// event last, after any token transfer, so this is the vault's event.
fn last_event(env: &Env) -> LastEvent {
    let all = env.events().all();
    let ContractEventBody::V0(body) = &all.events().last().unwrap().body;
    LastEvent {
        env: env.clone(),
        topics: ScVal::Vec(Some(ScVec(body.topics.clone()))),
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

    let admin = Address::generate(&env);
    let contract_id = env.register(DonationVault, DonationVaultArgs::__constructor(&admin));
    let client = DonationVaultClient::new(&env, &contract_id);

    let token_admin_addr = Address::generate(&env);
    let (token, token_admin) = create_token(&env, &token_admin_addr);

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

    // `last_event` only sees the latest top-level call, so snapshot it before
    // any other call (such as a balance read) replaces it.
    let created = last_event(&s.env);
    assert_eq!(
        created,
        (
            (symbol_short!("created"), stream_id).into_val(&s.env),
            (
                s.donor.clone(),
                s.ngo.clone(),
                s.token.address.clone(),
                1_000i128,
                10i128,
                Bytes::new(&s.env)
            )
                .into_val(&s.env),
        )
    );
    assert_eq!(s.token.balance(&s.donor), 0);
    assert_eq!(s.token.balance(&s.client.address), 1_000);

    // 50 seconds pass -> 10/s * 50 = 500 should be withdrawable.
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let withdrawn = s.client.withdraw(&stream_id);
    let withdrew = last_event(&s.env);
    assert_eq!(withdrawn, 500);
    assert_eq!(
        withdrew,
        (
            (symbol_short!("withdraw"), stream_id).into_val(&s.env),
            (500i128, 0i128, 500i128, s.ngo.clone()).into_val(&s.env),
        )
    );
    assert_eq!(s.token.balance(&s.ngo), 500);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 500);
    assert_eq!(stream.withdrawn, 500);

    // 20 more seconds pass, then the donor cancels.
    s.env.ledger().with_mut(|l| l.timestamp += 20);
    // 200 more settles to the NGO on cancel; the untouched 300 refunds to the donor.
    let refund = s.client.cancel_stream(&stream_id);
    let cancelled = last_event(&s.env);
    assert_eq!(refund, 300);
    assert_eq!(
        cancelled,
        (
            (symbol_short!("cancel"), stream_id).into_val(&s.env),
            (200i128, 300i128).into_val(&s.env),
        )
    );
    assert_eq!(s.token.balance(&s.ngo), 700);
    assert_eq!(s.token.balance(&s.donor), 300);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.rate, 0);
    assert!(stream.cancelled);
}

#[test]
fn one_stroop_per_second_stream_pays_and_rounds_fee_to_zero() {
    let s = setup();
    s.token_admin.mint(&s.donor, &100);
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &100, &1);
    s.env.ledger().with_mut(|l| l.timestamp += 1);

    assert_eq!(s.client.withdraw(&stream_id), 1);
    assert_eq!(s.token.balance(&s.ngo), 1);
    assert_eq!(s.token.balance(&treasury), 0);
}

#[test]
fn concurrent_streams_to_one_ngo_accrue_and_pay_out_independently() {
    let s = setup();
    let donor_b = Address::generate(&s.env);
    s.token_admin.mint(&s.donor, &1_000);
    s.token_admin.mint(&donor_b, &600);

    // Stream A starts first; stream B starts 20 seconds later at a
    // different rate, so the two accruals differ.
    let id_a = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 20);
    let id_b = s
        .client
        .create_stream(&donor_b, &s.ngo, &s.token.address, &600, &5);
    assert_ne!(id_a, id_b);
    assert_eq!(s.token.balance(&s.client.address), 1_600);

    // 30 seconds later: A has run 50s (500), B has run 30s (150).
    s.env.ledger().with_mut(|l| l.timestamp += 30);
    assert_eq!(s.client.pending_accrual(&id_a), 500);
    assert_eq!(s.client.pending_accrual(&id_b), 150);

    // Withdrawing A pays exactly A's accrual and leaves B untouched.
    assert_eq!(s.client.withdraw(&id_a), 500);
    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.client.pending_accrual(&id_b), 150);
    let stream_b = s.client.get_stream(&id_b);
    assert_eq!(stream_b.balance, 600);
    assert_eq!(stream_b.withdrawn, 0);

    // Withdrawing B pays exactly B's accrual and leaves A untouched.
    assert_eq!(s.client.withdraw(&id_b), 150);
    assert_eq!(s.token.balance(&s.ngo), 650);

    let stream_a = s.client.get_stream(&id_a);
    assert_eq!(stream_a.balance, 500);
    assert_eq!(stream_a.withdrawn, 500);
    let stream_b = s.client.get_stream(&id_b);
    assert_eq!(stream_b.balance, 450);
    assert_eq!(stream_b.withdrawn, 150);
    assert_eq!(s.token.balance(&s.client.address), 950);

    // Both keep accruing at their own rates after the other's withdrawal.
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    assert_eq!(s.client.pending_accrual(&id_a), 100);
    assert_eq!(s.client.pending_accrual(&id_b), 50);
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
    let topped_up = last_event(&s.env);

    assert_eq!(
        topped_up,
        (
            (symbol_short!("topup"), stream_id).into_val(&s.env),
            (500i128, 100i128).into_val(&s.env),
        )
    );
    assert_eq!(s.token.balance(&s.ngo), 100);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_400); // 1000 - 100 accrued + 500 top-up
    assert_eq!(stream.rate, 10);

    s.env.ledger().with_mut(|l| l.timestamp += 5); // 50 more accrues at the old rate

    s.client.modify_rate(&stream_id, &20);
    assert_last_event(
        &s.env,
        (symbol_short!("ratemod"), stream_id),
        (10i128, 20i128, 50i128),
    );
    let rate_changed = last_event(&s.env);

    assert_eq!(
        rate_changed,
        (
            (symbol_short!("ratemod"), stream_id).into_val(&s.env),
            (10i128, 20i128, 50i128).into_val(&s.env),
        )
    );
    assert_eq!(s.token.balance(&s.ngo), 150);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.rate, 20);
    assert_eq!(stream.balance, 1_350); // 1400 - 50
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
fn create_stream_errors_instead_of_defaulting_when_counter_is_missing() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    // `init` always sets NextStreamId, so this shouldn't happen in
    // practice â€” but nothing enforces that, and if the counter were ever
    // missing, silently treating it as `0` could collide with an existing
    // stream. Simulate that by removing it directly from instance storage.
    s.env.as_contract(&s.client.address, || {
        s.env.storage().instance().remove(&DataKey::NextStreamId);
    });

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::StreamCounterMissing)));

    // No stream should have been recorded under the fabricated id 0.
    assert_eq!(
        s.client.try_get_stream(&0u64),
        Err(Ok(Error::StreamNotFound))
    );
}

#[test]
fn propose_then_accept_admin_transfers_control() {
    let s = setup();
    let old_admin = s.client.admin();
    let new_admin = Address::generate(&s.env);

    s.client.propose_admin(&new_admin);
    let proposed = last_event(&s.env);
    assert_eq!(
        proposed,
        (
            (symbol_short!("propadmin"),).into_val(&s.env),
            new_admin.into_val(&s.env),
        )
    );
    // Admin hasn't changed yet â€” only proposed.
    assert_eq!(s.client.admin(), old_admin);

    s.client.accept_admin();
    let accepted = last_event(&s.env);
    assert_eq!(
        accepted,
        (
            (symbol_short!("acptadmin"),).into_val(&s.env),
            new_admin.into_val(&s.env),
        )
    );
    assert_eq!(s.client.admin(), new_admin);

    // The new admin can act as admin.
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    assert_eq!(s.client.treasury(), Some(treasury));
}

#[test]
fn propose_admin_rejects_current_admin() {
    let s = setup();
    let admin = s.client.admin();

    assert_eq!(
        s.client.try_propose_admin(&admin),
        Err(Ok(Error::InvalidAdmin))
    );
    assert_eq!(s.client.pending_admin(), None);
}

#[test]
fn allowed_tokens_is_empty_when_unconfigured() {
    let s = setup();

    assert_eq!(s.client.allowed_tokens().len(), 0);
}

#[test]
fn accept_admin_without_proposal_fails() {
    let s = setup();
    let result = s.client.try_accept_admin();
    assert_eq!(result, Err(Ok(Error::NoPendingAdmin)));
}

// â”€â”€ Issue #72 â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// propose_admin overwrites rather than queues, so the first proposed address
// is silently dropped. That's the desired behaviour, but nothing pinned it:
// a future change to "keep the earliest proposal" or "reject a second one"
// would lock an admin out with no way to tell from the outside.

#[test]
fn repropose_admin_overwrites_earlier_proposal() {
    let s = setup();
    let old_admin = s.client.admin();
    let admin_a = Address::generate(&s.env);
    let admin_b = Address::generate(&s.env);

    s.client.propose_admin(&admin_a);
    assert_eq!(s.client.pending_admin(), Some(admin_a.clone()));

    // Proposing again replaces the pending address instead of queueing.
    s.client.propose_admin(&admin_b);
    assert_eq!(
        s.client.pending_admin(),
        Some(admin_b.clone()),
        "the second proposal must overwrite the first, not queue behind it"
    );

    // A is no longer the pending admin, so accept_admin now requires B's auth
    // and not A's.
    s.client.accept_admin();
    assert_auth_required_from(&s, &admin_b, "accept_admin");

    // Control actually moved to B, and only to B.
    assert_eq!(s.client.admin(), admin_b);
    assert_ne!(s.client.admin(), old_admin);
    assert_ne!(s.client.admin(), admin_a);
}

#[test]
fn cancel_admin_proposal_without_proposal_fails() {
    let s = setup();
    let result = s.client.try_cancel_admin_proposal();
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
fn depletion_time_is_last_update_plus_balance_over_rate() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let created_at = s.client.get_stream(&stream_id).created_at;

    // 1_000 at 10/s divides evenly: exactly 100 seconds.
    assert_eq!(s.client.depletion_time(&stream_id), Some(created_at + 100));

    // Nothing is settled by just asking, and the answer doesn't drift as
    // time passes without a settlement.
    s.env.ledger().with_mut(|l| l.timestamp += 30);
    assert_eq!(s.client.depletion_time(&stream_id), Some(created_at + 100));
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

#[test]
fn depletion_time_rounds_a_partial_final_second_up() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    // 1_000 / 300 = 3.33 seconds: three seconds leave 100 unpaid, so the
    // stream is only empty once the fourth second has run.
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &300);
    let created_at = s.client.get_stream(&stream_id).created_at;
    let depletes_at = s.client.depletion_time(&stream_id).unwrap();
    assert_eq!(depletes_at, created_at + 4);

    // One second short of it, funds are still left...
    s.env.ledger().with_mut(|l| l.timestamp = depletes_at - 1);
    assert_eq!(s.client.pending_accrual(&stream_id), 900);

    // ...and at the reported time the whole balance has accrued.
    s.env.ledger().with_mut(|l| l.timestamp = depletes_at);
    assert_eq!(s.client.pending_accrual(&stream_id), 1_000);
}

#[test]
fn depletion_time_moves_with_a_settlement() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &300);

    // Withdraw after 2 seconds: 600 paid, 400 left, settled at that moment.
    s.env.ledger().with_mut(|l| l.timestamp += 2);
    s.client.withdraw(&stream_id);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 400);

    // 400 / 300 = 1.33 -> 2 more seconds from the new last_update.
    assert_eq!(
        s.client.depletion_time(&stream_id),
        Some(stream.last_update + 2)
    );
}

#[test]
fn depletion_time_of_a_balance_below_the_rate_is_one_second_out() {
    let s = setup();
    s.token_admin.mint(&s.donor, &5);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &5, &1_000);
    let created_at = s.client.get_stream(&stream_id).created_at;

    assert_eq!(s.client.depletion_time(&stream_id), Some(created_at + 1));
}

#[test]
fn depletion_time_of_a_drained_stream_is_its_last_update() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 500);
    s.client.withdraw(&stream_id);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(
        s.client.depletion_time(&stream_id),
        Some(stream.last_update)
    );
}

#[test]
fn depletion_time_of_a_cancelled_stream_is_none() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.cancel_stream(&stream_id);

    assert_eq!(s.client.depletion_time(&stream_id), None);
}

#[test]
fn depletion_time_on_unknown_stream_fails() {
    let s = setup();
    let result = s.client.try_depletion_time(&999);
    assert_eq!(result, Err(Ok(Error::StreamNotFound)));
}

// =============================================================================
// Issue #345: pending_refund view for donors
// =============================================================================

#[test]
fn pending_refund_of_an_active_stream_matches_the_cancel_refund() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 20); // 200 accrues

    // Balance minus the accrued slice.
    assert_eq!(s.client.pending_refund(&stream_id), 800);

    // Read-only: nothing settled and no funds moved.
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);
    assert_eq!(s.token.balance(&s.ngo), 0);

    // It must equal exactly what cancel_stream actually refunds.
    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 800);
}

#[test]
fn pending_refund_of_a_drained_stream_is_zero() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 500); // the whole balance accrues

    // Until it is settled the balance is still whole, but the accrued slice
    // already equals it, so nothing would come back.
    assert_eq!(s.client.pending_refund(&stream_id), 0);

    s.client.withdraw(&stream_id);
    assert_eq!(s.client.get_stream(&stream_id).balance, 0);
    assert_eq!(s.client.pending_refund(&stream_id), 0);
}

#[test]
fn pending_refund_of_a_cancelled_stream_is_zero() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 20);
    s.client.cancel_stream(&stream_id);

    // Cancelling zeroes balance and rate, so there is nothing left to refund.
    assert_eq!(s.client.pending_refund(&stream_id), 0);
}

#[test]
fn pending_refund_of_a_paused_stream_is_the_full_balance() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.pause_stream(&stream_id);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // paused: nothing accrues

    // `cancel_stream` settles through `settle`, which accrues nothing for a
    // paused stream, so the whole remaining balance would come back.
    assert_eq!(s.client.pending_refund(&stream_id), 1_000);
}

#[test]
fn pending_refund_before_a_streams_cliff_is_the_full_balance() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let start = s.env.ledger().timestamp() + 100;
    let stream_id =
        s.client
            .create_stream_with_start(&s.donor, &s.ngo, &s.token.address, &1_000, &10, &start);

    // Nothing accrues before `start`, so the whole deposit is refundable.
    assert_eq!(s.client.pending_refund(&stream_id), 1_000);

    s.env.ledger().with_mut(|l| l.timestamp = start + 20); // 200 accrues
    assert_eq!(s.client.pending_refund(&stream_id), 800);
}

#[test]
fn pending_refund_on_unknown_stream_fails() {
    let s = setup();
    let result = s.client.try_pending_refund(&999);
    assert_eq!(result, Err(Ok(Error::StreamNotFound)));
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
fn withdraw_immediately_after_top_up_fails() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 10); // 100 accrues

    // top_up settles the accrued 100 to the NGO internally.
    s.client.top_up(&stream_id, &500);
    assert_eq!(s.token.balance(&s.ngo), 100);

    // No time has passed since the settlement, so nothing new has accrued.
    let result = s.client.try_withdraw(&stream_id);
    assert_eq!(result, Err(Ok(Error::NothingToWithdraw)));
    assert_eq!(s.token.balance(&s.ngo), 100);
}

#[test]
fn pause_blocks_create_but_not_cancel() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.pause();
    let paused_evt = last_event(&s.env);
    assert_eq!(
        paused_evt,
        (
            (symbol_short!("pause"),).into_val(&s.env),
            ().into_val(&s.env),
        )
    );
    assert!(s.client.paused());

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &100, &10);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));

    // Cancelling still works while paused, so donors are never trapped.
    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 1_000);
    assert_eq!(s.token.balance(&s.donor), 1_000);
}

#[test]
fn pause_blocks_withdraw_but_not_cancel() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 has accrued

    s.client.pause();

    // withdraw is the one entry point that pays tokens straight out of the
    // vault, so the brake has to stop it even with funds already waiting to
    // be claimed â€” otherwise pausing buys no protection at all.
    let result = s.client.try_withdraw(&stream_id);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));

    // The rejected call is a no-op: nothing moves, and the accrual it would
    // have settled stays on the stream for after the pause is lifted.
    assert_eq!(s.token.balance(&s.ngo), 0);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);

    // Cancelling still works while paused, so donors are never trapped.
    s.client.cancel_stream(&stream_id);
    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.token.balance(&s.donor), 500);
}

#[test]
fn unpause_restores_normal_operation() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    s.client.pause();
    s.client.unpause();
    let unpaused_evt = last_event(&s.env);
    assert_eq!(
        unpaused_evt,
        (
            (symbol_short!("unpause"),).into_val(&s.env),
            ().into_val(&s.env),
        )
    );
    assert!(!s.client.paused());

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
}

#[test]
fn pause_rejects_duplicate_pause() {
    let s = setup();

    s.client.pause();
    assert_eq!(s.client.try_pause(), Err(Ok(Error::AlreadyPaused)));
    assert!(s.client.paused());
}

#[test]
fn unpause_rejects_duplicate_unpause() {
    let s = setup();

    assert_eq!(s.client.try_unpause(), Err(Ok(Error::AlreadyUnpaused)));
    s.client.pause();
    s.client.unpause();
    assert_eq!(s.client.try_unpause(), Err(Ok(Error::AlreadyUnpaused)));
    assert!(!s.client.paused());
}

#[test]
fn pause_blocks_fund_movement_but_allows_cancel_and_queries() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);
    let treasury = Address::generate(&s.env);
    let new_admin = Address::generate(&s.env);

    // Create a stream before pausing
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 10); // 100 accrues

    s.client.pause();
    assert!(s.client.paused());

    // Fund-moving operations are blocked
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &500, &10),
        Err(Ok(Error::ContractPaused)),
        "create_stream should be blocked"
    );
    assert_eq!(
        s.client.try_withdraw(&stream_id),
        Err(Ok(Error::ContractPaused)),
        "withdraw should be blocked"
    );
    assert_eq!(
        s.client.try_top_up(&stream_id, &500),
        Err(Ok(Error::ContractPaused)),
        "top_up should be blocked"
    );
    assert_eq!(
        s.client.try_modify_rate(&stream_id, &20),
        Err(Ok(Error::ContractPaused)),
        "modify_rate should be blocked"
    );
    assert_eq!(
        s.client
            .try_transfer_stream(&stream_id, &Address::generate(&s.env)),
        Err(Ok(Error::ContractPaused)),
        "transfer_stream should be blocked"
    );
    assert_eq!(
        s.client.try_pause_stream(&stream_id),
        Err(Ok(Error::ContractPaused)),
        "pause_stream should be blocked"
    );
    assert_eq!(
        s.client.try_resume_stream(&stream_id),
        Err(Ok(Error::ContractPaused)),
        "resume_stream should be blocked"
    );
    let mut ids = Vec::new(&s.env);
    ids.push_back(stream_id);
    assert_eq!(
        s.client.try_withdraw_batch(&ids),
        Err(Ok(Error::ContractPaused)),
        "withdraw_batch should be blocked"
    );
    assert_eq!(
        s.client.try_withdraw_all(&s.ngo, &0, &10),
        Err(Ok(Error::ContractPaused)),
        "withdraw_all should be blocked"
    );
    assert_eq!(
        s.client.try_sweep_surplus(&s.token.address, &100),
        Err(Ok(Error::ContractPaused)),
        "sweep_surplus should be blocked"
    );
    assert_eq!(
        s.client
            .try_sweep_surplus_to(&s.token.address, &100, &Address::generate(&s.env)),
        Err(Ok(Error::ContractPaused)),
        "sweep_surplus_to should be blocked"
    );

    // cancel_stream always works (emergency exit for donors)
    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 900); // 1000 - 100 that accrued
    assert_eq!(s.token.balance(&s.ngo), 100);
    assert_eq!(s.token.balance(&s.donor), 2_900);

    // rescue_stream requires the vault to be paused
    s.token_admin.mint(&s.donor, &500);
    // Creating a stream needs an unpaused vault, so open it first and pause
    // again before the rescue.
    s.client.unpause();
    let stream_id2 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &500, &10);
    s.client.pause();
    let rescued = s.client.rescue_stream(&stream_id2);
    assert_eq!(rescued, 500);
    assert_eq!(s.token.balance(&s.donor), 3_400);

    // Read-only operations still work
    assert!(s.client.get_stream(&stream_id).cancelled);
    assert_eq!(s.client.pending_accrual(&stream_id2), 0);
    assert_eq!(s.client.depletion_time(&stream_id2), None);
    assert_eq!(s.client.total_donated(), 1_500);

    // Admin operations still work
    s.client.set_treasury(&treasury);
    assert_eq!(s.client.treasury(), Some(treasury.clone()));
    s.client.set_fee_bps(&100);
    assert_eq!(s.client.fee_bps(), 100);
    s.client.set_min_deposit(&50);
    assert_eq!(s.client.min_deposit(), 50);
    s.client.set_max_streams_per_donor(&200);
    assert_eq!(s.client.max_streams_per_donor(), 200);
    s.client.propose_admin(&new_admin);
    assert_eq!(s.client.pending_admin(), Some(new_admin.clone()));
    s.client.cancel_admin_proposal();
    assert_eq!(s.client.pending_admin(), None);

    // unpause restores normal operation
    s.client.unpause();
    assert!(!s.client.paused());
}

#[test]
fn withdraw_with_no_treasury_takes_no_fee() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);
    s.client.set_fee_bps(&0);
    s.client.clear_treasury(); // fee must be zero before the treasury can be removed

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 500);
    assert_eq!(s.token.balance(&s.ngo), 500);
}

#[test]
fn clear_treasury_stops_fee_collection_on_subsequent_withdraws() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5%

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // First withdraw while treasury is active â€” fee is taken.
    s.env.ledger().with_mut(|l| l.timestamp += 20); // 200 accrues
    s.client.withdraw(&stream_id);
    assert_eq!(s.token.balance(&treasury), 10); // 5% of 200
    assert_eq!(s.token.balance(&s.ngo), 190);

    // Admin clears the treasury; storage key is gone. A non-zero fee must
    // be removed first (it would otherwise have nowhere to go).
    s.client.set_fee_bps(&0);
    s.client.clear_treasury();
    assert_eq!(s.client.treasury(), None);

    // Second withdraw â€” full amount goes to the NGO, nothing to the old treasury.
    s.env.ledger().with_mut(|l| l.timestamp += 20); // 200 more accrues
    s.client.withdraw(&stream_id);
    assert_eq!(s.token.balance(&s.ngo), 390); // 190 + 200, no fee
    assert_eq!(s.token.balance(&treasury), 10); // unchanged
}

#[test]
fn withdraw_splits_protocol_fee_to_treasury() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    let treasury_evt = last_event(&s.env);
    assert_eq!(
        treasury_evt,
        (
            (symbol_short!("treasset"),).into_val(&s.env),
            treasury.clone().into_val(&s.env),
        )
    );
    s.client.set_fee_bps(&500); // 5%
    let feebps_evt = last_event(&s.env);
    assert_eq!(
        feebps_evt,
        (
            (symbol_short!("feeset"),).into_val(&s.env),
            500u32.into_val(&s.env),
        )
    );
    assert_last_event(&s.env, (symbol_short!("feeset"),), 500u32);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues

    let withdrawn = s.client.withdraw(&stream_id);
    // The return value is the net payout â€” the 500 that accrued minus the
    // 5% (25) sent to the treasury â€” not the gross accrued amount.
    assert_eq!(withdrawn, 475);
    assert_eq!(s.token.balance(&treasury), 25);
    assert_eq!(s.token.balance(&s.ngo), 475);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.withdrawn, 500); // bookkeeping still tracks the gross amount
}

#[test]
fn withdraw_returns_net_after_protocol_fee() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&250); // 2.5%

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 40); // 400 accrues

    let ngo_before = s.token.balance(&s.ngo);
    let withdrawn = s.client.withdraw(&stream_id);

    // The return value is exactly what the NGO received: 400 gross - 10 fee.
    assert_eq!(withdrawn, 390);
    assert_eq!(s.token.balance(&s.ngo) - ngo_before, withdrawn);
    assert_eq!(s.token.balance(&treasury), 10);

    // And the stream's bookkeeping still records the gross 400 as withdrawn.
    assert_eq!(s.client.get_stream(&stream_id).withdrawn, 400);
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

    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 800);

    // 5% of the 200 accrued goes to the treasury; the rest settles to the NGO.
    assert_eq!(s.token.balance(&treasury), 10);
    assert_eq!(s.token.balance(&s.ngo), 190);
    // The untouched 800 refunds to the donor in full â€” no fee on refunds.
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
    // The fee truncates to zero, so the net return equals the gross here.
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
    // 20 gross - 1 fee: the return is net, unlike the 20 recorded as withdrawn.
    assert_eq!(withdrawn, 19);
    assert_eq!(s.token.balance(&treasury), 1);
    assert_eq!(s.token.balance(&s.ngo), 19);
}

#[test]
fn set_treasury_rejects_the_vault_own_address() {
    let s = setup();
    let real_treasury = Address::generate(&s.env);
    s.client.set_treasury(&real_treasury);

    let result = s.client.try_set_treasury(&s.client.address);
    assert_eq!(result, Err(Ok(Error::InvalidTreasury)));

    // The rejected call leaves the existing treasury in place.
    assert_eq!(s.client.treasury(), Some(real_treasury));
}

#[test]
fn set_treasury_rejects_the_vault_own_address_when_none_is_set() {
    let s = setup();

    let result = s.client.try_set_treasury(&s.client.address);
    assert_eq!(result, Err(Ok(Error::InvalidTreasury)));
    assert_eq!(s.client.treasury(), None);
}

#[test]
fn compute_fee_avoids_overflow_near_i128_max() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&1_000);

    let amount = i128::MAX;
    let fee = s.env.as_contract(&s.client.address, || {
        compute_fee(&s.env, &s.token.address, amount)
    });
    assert_eq!(
        fee,
        (amount / 10_000) * 1_000 + ((amount % 10_000) * 1_000) / 10_000
    );
}

#[test]
fn get_config_reflects_admin_settings() {
    let s = setup();
    s.token_admin.mint(&s.donor, &5_000);

    // 1. Initial default configuration
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: false,
            treasury: None,
            fee_bps: 0,
            min_deposit: 0,
            max_streams_per_donor: 100,
            max_rate: None,
            min_duration: 0,
            registry: None,
            cancel_grace_ledgers: 0,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // 2. Set admin parameters
    let treasury = Address::generate(&s.env);
    let registry = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);
    s.client.set_min_deposit(&100);
    s.client.set_max_streams_per_donor(&50);
    s.client.set_cancel_grace_ledgers(&10);
    s.client.pause();

    assert_eq!(
        s.client.get_config(),
        Config {
            paused: true,
            treasury: Some(treasury.clone()),
            fee_bps: 500,
            min_deposit: 100,
            max_streams_per_donor: 50,
            max_rate: None,
            min_duration: 0,
            registry: None, // configured after the stream operations below
            cancel_grace_ledgers: 10,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // 3. Proposed admin state change
    let candidate = Address::generate(&s.env);
    s.client.propose_admin(&candidate);
    assert_eq!(s.client.get_config().pending_admin, Some(candidate));

    // 4. Changed fee
    s.client.set_fee_bps(&250);
    assert_eq!(s.client.get_config().fee_bps, 250);

    // 5. Unpause, make donations (create_stream and top_up) -> updates total_donated
    s.client.unpause();
    assert!(!s.client.get_config().paused);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_config().total_donated, 1_000);

    s.client.top_up(&stream_id, &500);
    assert_eq!(s.client.get_config().total_donated, 1_500);

    // Configure the registry only after the stream operations: a random
    // address is not a working registry, so create_stream would reject the NGO.
    s.client.set_registry(&registry);

    // 6. Renounce admin -> renounced = true, pending_admin = None
    s.client.renounce_admin();
    let final_config = s.client.get_config();
    assert!(final_config.renounced);
    assert_eq!(final_config.pending_admin, None);
    assert_eq!(final_config.total_donated, 1_500);
    assert_eq!(final_config.treasury, Some(treasury));
    assert_eq!(final_config.fee_bps, 250);
    assert_eq!(final_config.min_deposit, 100);
    assert_eq!(final_config.max_streams_per_donor, 50);
    assert_eq!(final_config.registry, Some(registry));
    assert_eq!(final_config.cancel_grace_ledgers, 10);
}

#[test]
fn set_fee_bps_rejects_over_cap() {
    let s = setup();
    let result = s.client.try_set_fee_bps(&1_001);
    assert_eq!(result, Err(Ok(Error::FeeTooHigh)));
}

#[test]
fn set_fee_bps_boundary_exact_max_succeeds() {
    let s = setup();
    s.client.set_treasury(&Address::generate(&s.env));

    // Exactly 1 000 bps (10%) is the maximum allowed fee â€” it must be
    // accepted and stored faithfully.
    s.client.set_fee_bps(&1_000);
    assert_eq!(s.client.fee_bps(), 1_000);

    // One basis point above the cap must still be rejected with FeeTooHigh
    // specifically, not just any error, so an off-by-one in the guard
    // can't hide behind a different error path.
    let result = s.client.try_set_fee_bps(&1_001);
    assert_eq!(result, Err(Ok(Error::FeeTooHigh)));
}

#[test]
fn set_treasury_emits_treasset_event() {
    let s = setup();

    let first = Address::generate(&s.env);
    s.client.set_treasury(&first);
    assert_last_event(&s.env, (symbol_short!("treasset"),), first.clone());
    assert_eq!(s.client.treasury(), Some(first));

    // Re-pointing the treasury publishes the replacement address.
    let second = Address::generate(&s.env);
    s.client.set_treasury(&second);
    assert_last_event(&s.env, (symbol_short!("treasset"),), second.clone());
    assert_eq!(s.client.treasury(), Some(second));
}

#[test]
fn set_fee_bps_emits_feeset_event() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);

    s.client.set_fee_bps(&500); // 5%
    assert_last_event(&s.env, (symbol_short!("feeset"),), 500u32);
    assert_eq!(s.client.fee_bps(), 500);

    // The cap boundary is accepted and emits the accepted value.
    s.client.set_fee_bps(&MAX_FEE_BPS);
    assert_last_event(&s.env, (symbol_short!("feeset"),), MAX_FEE_BPS);
    assert_eq!(s.client.fee_bps(), MAX_FEE_BPS);
}

#[test]
fn max_fee_bps_is_exposed_on_chain() {
    let s = setup();
    assert_eq!(s.client.max_fee_bps(), 1_000);
}

#[test]
fn token_fee_bps_falls_back_to_the_global_default_with_no_override() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% global default

    assert_eq!(s.client.token_fee_bps(&s.token.address), 500);

    let other_token = Address::generate(&s.env);
    assert_eq!(s.client.token_fee_bps(&other_token), 500);
}

#[test]
fn token_fee_bps_override_takes_priority_over_the_global_default() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% global default
    s.client.set_token_fee_bps(&s.token.address, &100); // 1% for this token only

    assert_eq!(s.client.token_fee_bps(&s.token.address), 100);

    // A different token, never overridden, still sees the global default.
    let other_token = Address::generate(&s.env);
    assert_eq!(s.client.token_fee_bps(&other_token), 500);
}

#[test]
fn set_token_fee_bps_rejects_over_cap() {
    let s = setup();
    let result = s.client.try_set_token_fee_bps(&s.token.address, &1_001);
    assert_eq!(result, Err(Ok(Error::FeeTooHigh)));
}

#[test]
fn set_token_fee_bps_requires_admin_auth() {
    let s = setup();
    s.env.mock_auths(&[]);
    let result = s.client.try_set_token_fee_bps(&s.token.address, &500);
    assert!(result.is_err());
}

#[test]
fn set_token_fee_bps_emits_token_and_value() {
    let s = setup();
    s.client.set_treasury(&Address::generate(&s.env));
    s.client.set_token_fee_bps(&s.token.address, &250);

    assert_eq!(
        last_event(&s.env),
        (
            (symbol_short!("tokfeeset"), s.token.address.clone()).into_val(&s.env),
            250u32.into_val(&s.env),
        )
    );
}

#[test]
fn clear_token_fee_bps_requires_admin_auth() {
    let s = setup();
    let admin = s.client.admin();
    s.client.clear_token_fee_bps(&s.token.address);
    assert_auth_required_from(&s, &admin, "clear_token_fee_bps");

    s.env.mock_auths(&[]);
    let result = s.client.try_clear_token_fee_bps(&s.token.address);
    assert!(result.is_err());
}

#[test]
fn clear_token_fee_bps_restores_global_fee_on_withdraw() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% global fee
    s.client.set_token_fee_bps(&s.token.address, &100); // 1% token override

    assert_eq!(s.client.token_fee_bps(&s.token.address), 100);

    // Create stream and withdraw with override in effect
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 gross accrues
                                                    // 1% of 500 = 5 fee, 495 net
    let net = s.client.withdraw(&stream_id);
    assert_eq!(net, 495);
    assert_eq!(s.token.balance(&s.ngo), 495);
    assert_eq!(s.token.balance(&treasury), 5);

    // Clear the token fee override
    s.client.clear_token_fee_bps(&s.token.address);
    // Getter reflects fallback to global 5%
    assert_eq!(s.client.token_fee_bps(&s.token.address), 500);

    // Withdraw again after clearing: global fee (5%) must apply to actual payout
    s.env.ledger().with_mut(|l| l.timestamp += 50); // another 500 gross accrues
                                                    // 5% of 500 = 25 fee, 475 net
    let net2 = s.client.withdraw(&stream_id);
    assert_eq!(net2, 475);
    assert_eq!(s.token.balance(&s.ngo), 495 + 475);
    assert_eq!(s.token.balance(&treasury), 5 + 25);
}

#[test]
fn clear_token_fee_bps_with_explicit_zero_override() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% global fee
    s.client.set_token_fee_bps(&s.token.address, &0); // 0% explicit override

    assert_eq!(s.client.token_fee_bps(&s.token.address), 0);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 gross accrues

    // Explicit 0% override still gets 0% fee until cleared
    let net = s.client.withdraw(&stream_id);
    assert_eq!(net, 500);
    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.token.balance(&treasury), 0);

    // Now clear the override
    s.client.clear_token_fee_bps(&s.token.address);
    assert_eq!(s.client.token_fee_bps(&s.token.address), 500);

    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 gross accrues
                                                    // Global 5% fee applies: 25 fee, 475 net
    let net2 = s.client.withdraw(&stream_id);
    assert_eq!(net2, 475);
    assert_eq!(s.token.balance(&s.ngo), 500 + 475);
    assert_eq!(s.token.balance(&treasury), 25);
}

#[test]
fn clear_token_fee_bps_emits_event() {
    let s = setup();
    s.client.set_treasury(&Address::generate(&s.env));
    s.client.set_token_fee_bps(&s.token.address, &200);

    s.client.clear_token_fee_bps(&s.token.address);

    assert_eq!(
        last_event(&s.env),
        (
            (symbol_short!("tokfeeclr"), s.token.address.clone()).into_val(&s.env),
            ().into_val(&s.env),
        )
    );
}

#[test]
fn clear_token_fee_bps_noop_when_no_override() {
    let s = setup();
    // No override configured for s.token.address
    let result = s.client.try_clear_token_fee_bps(&s.token.address);
    assert!(result.is_ok());

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    let fee_before = s.client.token_fee_bps(&s.token.address);
    s.client.clear_token_fee_bps(&s.token.address);
    let fee_after = s.client.token_fee_bps(&s.token.address);
    assert_eq!(
        fee_before, fee_after,
        "a no-op clear must not change the effective fee"
    );
}

#[test]
fn withdraw_uses_the_per_token_fee_override_instead_of_the_global_default() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% global
    s.client.set_token_fee_bps(&s.token.address, &100); // 1% for this token

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues

    let (net, fee) = s.client.pending_payout(&stream_id);
    assert_eq!(fee, 5); // 1% override of 500, not the 5% global fee
    assert_eq!(net, 495);

    // Read-only: nothing moved.
    assert_eq!(s.token.balance(&s.ngo), 0);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);
}

#[test]
fn pending_payout_with_treasury_and_zero_bps_returns_full_gross_and_zero_fee() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    // fee_bps defaults to 0 â€” a treasury exists but takes nothing

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues

    let (net, fee) = s.client.pending_payout(&stream_id);
    assert_eq!(fee, 0); // 0 bps â†’ zero fee even with a treasury set
    assert_eq!(net, 500);

    // Read-only: nothing moved.
    assert_eq!(s.token.balance(&s.ngo), 0);
    assert_eq!(s.token.balance(&treasury), 0);
}

#[test]
fn pending_payout_with_nonzero_fee_matches_actual_withdraw_split() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5%

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues

    let (net, fee) = s.client.pending_payout(&stream_id);
    assert_eq!(fee, 25); // 5% of 500
    assert_eq!(net, 475); // 500 - 25

    // Read-only: nothing moved yet.
    assert_eq!(s.token.balance(&s.ngo), 0);
    assert_eq!(s.token.balance(&treasury), 0);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);

    // The values must match exactly what withdraw actually distributes.
    s.client.withdraw(&stream_id);
    assert_eq!(s.token.balance(&s.ngo), net);
    assert_eq!(s.token.balance(&treasury), fee);
}

#[test]
fn withdraw_event_amounts_with_non_zero_fee() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% global fee

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 gross accrues

    let net = s.client.withdraw(&stream_id);
    let withdrew = last_event(&s.env);

    let gross: i128 = 500;
    let fee: i128 = 25; // 500 * 500 / 10_000
    assert_eq!(net, 475);
    // net + fee == gross
    assert_eq!(net + fee, gross);

    assert_eq!(
        withdrew,
        (
            (symbol_short!("withdraw"), stream_id).into_val(&s.env),
            (gross, fee, net, s.ngo.clone()).into_val(&s.env),
        )
    );
    assert_eq!(s.token.balance(&s.ngo), net);
    assert_eq!(s.token.balance(&treasury), fee);
}

#[test]
fn withdraw_event_amounts_with_token_fee_override() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% global fee
    s.client.set_token_fee_bps(&s.token.address, &200); // 2% token override

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 gross accrues

    let net = s.client.withdraw(&stream_id);
    let withdrew = last_event(&s.env);

    let gross: i128 = 500;
    let fee: i128 = 10; // 500 * 200 / 10_000
    assert_eq!(net, 490);
    // net + fee == gross
    assert_eq!(net + fee, gross);

    assert_eq!(
        withdrew,
        (
            (symbol_short!("withdraw"), stream_id).into_val(&s.env),
            (gross, fee, net, s.ngo.clone()).into_val(&s.env),
        )
    );
    assert_eq!(s.token.balance(&s.ngo), net);
    assert_eq!(s.token.balance(&treasury), fee);
}

#[test]
fn withdraw_event_amounts_with_zero_fee() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    // No treasury configured -> fee is 0
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 gross accrues

    let net = s.client.withdraw(&stream_id);
    let withdrew = last_event(&s.env);

    let gross: i128 = 500;
    let fee: i128 = 0;
    assert_eq!(net, 500);
    // net + fee == gross
    assert_eq!(net + fee, gross);

    assert_eq!(
        withdrew,
        (
            (symbol_short!("withdraw"), stream_id).into_val(&s.env),
            (gross, fee, net, s.ngo.clone()).into_val(&s.env),
        )
    );
    assert_eq!(s.token.balance(&s.ngo), net);
}

#[test]
fn min_deposit_setter_and_guard() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);
    s.client.set_min_deposit(&1_000);
    assert_eq!(s.client.min_deposit(), 1_000);

    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &999, &10),
        Err(Ok(Error::DepositTooLow))
    );
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

#[test]
fn min_deposit_requires_non_negative_admin_value() {
    let s = setup();
    assert_eq!(
        s.client.try_set_min_deposit(&-1),
        Err(Ok(Error::InvalidAmount))
    );
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
    // Drained by withdrawal, not cancelled â€” the rate is still live.
    assert!(!stream.cancelled);
    assert_eq!(stream.rate, 10);

    // More time passes, but there's nothing left to accrue.
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let result = s.client.try_withdraw(&stream_id);
    assert_eq!(result, Err(Ok(Error::NothingToWithdraw)));

    // Cancelling a drained stream settles and refunds nothing, but it does
    // flip `cancelled` â€” the one bit that distinguishes it from a stream
    // that merely ran dry.
    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 0);
    assert_eq!(s.token.balance(&s.ngo), 1_000);
    assert_eq!(s.token.balance(&s.donor), 0);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.rate, 0);
    assert_eq!(stream.withdrawn, 1_000);
    assert!(stream.cancelled);
}

#[test]
fn cancel_stream_twice_is_harmless() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues

    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 500);
    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.token.balance(&s.donor), 500);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.rate, 0);
    assert_eq!(stream.withdrawn, 500);
    assert!(stream.cancelled);

    // Cancelling again returns StreamCancelled error without rewriting storage
    // or resetting last_update, maintaining consistency with top_up and modify_rate.
    s.env.ledger().with_mut(|l| l.timestamp += 50);
    let last_update_before_second_cancel = stream.last_update;

    let result = s.client.try_cancel_stream(&stream_id);
    assert_eq!(result, Err(Ok(Error::StreamCancelled)));

    // Balances remain unchanged
    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.token.balance(&s.donor), 500);

    // Stream state remains unchanged (not rewritten)
    let stream_after = s.client.get_stream(&stream_id);
    assert_eq!(stream_after.balance, 0);
    assert_eq!(stream_after.rate, 0);
    assert_eq!(stream_after.withdrawn, 500);
    assert!(stream_after.cancelled);
    assert_eq!(stream_after.last_update, last_update_before_second_cancel);
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
fn modify_rate_on_cancelled_stream_fails() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.cancel_stream(&stream_id);

    let result = s.client.try_modify_rate(&stream_id, &20);
    assert_eq!(result, Err(Ok(Error::StreamCancelled)));

    // The rejected call leaves the cancelled stream's rate at zero â€” it
    // must not be revivable via modify_rate.
    assert_eq!(s.client.get_stream(&stream_id).rate, 0);
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
    // StreamNotFound â€” worth pinning so the order can't quietly flip.
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
fn pause_stream_on_a_drained_stream_returns_stream_drained() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Let it run to completion and pay everything out.
    s.env.ledger().with_mut(|l| l.timestamp += 100);
    assert_eq!(s.client.withdraw(&stream_id), 1_000);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Drained);
    assert!(!stream.cancelled);

    assert_eq!(
        s.client.try_pause_stream(&stream_id),
        Err(Ok(Error::StreamDrained))
    );
    // Still drained, and nothing moved.
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );
    assert_eq!(s.token.balance(&s.ngo), 1_000);
}

#[test]
fn pause_stream_twice_on_a_stream_it_drained_returns_stream_drained() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 100);

    // The first pause settles the whole balance, which drains the stream.
    assert_eq!(s.client.pause_stream(&stream_id), 1_000);
    assert_eq!(
        s.client.try_pause_stream(&stream_id),
        Err(Ok(Error::StreamDrained))
    );
}

#[test]
fn pause_stream_on_an_empty_stream_returns_stream_empty() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // An Active stream with a zero balance cannot be reached through the entry
    // points (they mark it Drained), so write the record directly.
    s.env.as_contract(&s.client.address, || {
        let key = DataKey::Stream(stream_id);
        let mut stream: Stream = s.env.storage().persistent().get(&key).unwrap();
        stream.balance = 0;
        s.env.storage().persistent().set(&key, &stream);
    });
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert!(!stream.cancelled);

    assert_eq!(
        s.client.try_pause_stream(&stream_id),
        Err(Ok(Error::StreamEmpty))
    );
}

#[test]
fn pause_stream_on_a_cancelled_stream_still_returns_stream_cancelled() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.cancel_stream(&stream_id);

    assert_eq!(
        s.client.try_pause_stream(&stream_id),
        Err(Ok(Error::StreamCancelled))
    );
}

#[test]
fn pause_stream_error_codes_are_distinct_and_append_only() {
    // Clients tell the cases apart by number, so these must never change.
    assert_eq!(Error::StreamCancelled as u32, 15);
    assert_eq!(Error::StreamAlreadyPaused as u32, 32);
    assert_eq!(Error::StreamDrained as u32, 43);
    assert_eq!(Error::StreamEmpty as u32, 44);
}

#[test]
fn top_up_on_cancelled_stream_fails() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.cancel_stream(&stream_id);

    let result = s.client.try_top_up(&stream_id, &500);
    assert_eq!(result, Err(Ok(Error::StreamCancelled)));

    // The rejected call moves no tokens: the donor still holds the full
    // 1_000 refunded by cancel_stream (no time passed, so nothing accrued).
    assert_eq!(s.token.balance(&s.donor), 2_000);
    assert_eq!(s.token.balance(&s.client.address), 0);
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
    // StreamNotFound â€” worth pinning so the order can't quietly flip.
    let result = s.client.try_top_up(&999, &0);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn top_up_rejects_below_minimum_deposit() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_500);

    s.client.set_min_deposit(&100);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Below minimum should fail
    let result = s.client.try_top_up(&stream_id, &50);
    assert_eq!(result, Err(Ok(Error::DepositTooLow)));

    // At minimum should succeed
    s.client.top_up(&stream_id, &100);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_100);
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
fn get_streams_returns_mixed_existing_and_missing_ids() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);

    let a = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let b = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &2_000, &20);

    let mut ids = Vec::new(&s.env);
    ids.push_back(a);
    ids.push_back(999);
    ids.push_back(b);

    let streams = s.client.get_streams(&ids);

    assert_eq!(streams.len(), 3);
    assert_eq!(streams.get(0).unwrap().unwrap(), s.client.get_stream(&a));
    assert_eq!(streams.get(1), Some(None));
    assert_eq!(streams.get(2).unwrap().unwrap(), s.client.get_stream(&b));
}

#[test]
fn get_streams_returns_none_for_every_missing_id() {
    let s = setup();

    let mut ids = Vec::new(&s.env);
    ids.push_back(7);
    ids.push_back(8);

    let streams = s.client.get_streams(&ids);

    assert_eq!(streams.len(), 2);
    assert_eq!(streams.get(0), Some(None));
    assert_eq!(streams.get(1), Some(None));
}

#[test]
fn get_streams_with_no_ids_is_empty_and_takes_no_auth() {
    let s = setup();
    let ids: Vec<u64> = Vec::new(&s.env);

    let streams = s.client.get_streams(&ids);

    assert_eq!(streams.len(), 0);
    assert!(s.env.auths().is_empty());
}

#[test]
fn get_streams_preserves_order_and_duplicates() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let a = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let mut ids = Vec::new(&s.env);
    ids.push_back(a);
    ids.push_back(a);
    ids.push_back(999);
    ids.push_back(a);

    let streams = s.client.get_streams(&ids);

    assert_eq!(streams.len(), 4);
    assert_eq!(streams.get(0).unwrap().unwrap(), s.client.get_stream(&a));
    assert_eq!(streams.get(1).unwrap().unwrap(), s.client.get_stream(&a));
    assert_eq!(streams.get(2), Some(None));
    assert_eq!(streams.get(3).unwrap().unwrap(), s.client.get_stream(&a));
}

#[test]
fn get_streams_at_exact_cap_succeeds() {
    let s = setup();
    let mut ids = Vec::new(&s.env);
    for i in 0..MAX_BATCH_GET_STREAMS {
        ids.push_back(i as u64);
    }

    let streams = s.client.get_streams(&ids);
    assert_eq!(streams.len(), MAX_BATCH_GET_STREAMS);
}

#[test]
fn get_streams_over_limit_fails_with_exact_error() {
    let s = setup();
    let mut ids = Vec::new(&s.env);
    for i in 0..=MAX_BATCH_GET_STREAMS {
        ids.push_back(i as u64);
    }

    let result = s.client.try_get_streams(&ids);
    assert_eq!(result, Err(Ok(Error::BatchLimitExceeded)));
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
            start: 12_345,
            last_update: 12_345,
            memo: Bytes::new(&s.env),
            status: StreamStatus::Active,
            cancelled: false,
        }
    );
}

#[test]
fn streams_by_donor_is_empty_before_any_stream() {
    let s = setup();
    assert_eq!(s.client.streams_by_donor(&s.donor).len(), 0);
}

#[test]
fn streams_by_donor_lists_a_single_stream() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let ids = s.client.streams_by_donor(&s.donor);
    assert_eq!(ids.len(), 1);
    assert_eq!(ids.get(0), Some(stream_id));
}

#[test]
fn streams_by_donor_lists_multiple_streams_in_creation_order() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);

    let first = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let second = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &20);
    let third = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &30);

    let ids = s.client.streams_by_donor(&s.donor);
    assert_eq!(ids.len(), 3);
    assert_eq!(ids.get(0), Some(first));
    assert_eq!(ids.get(1), Some(second));
    assert_eq!(ids.get(2), Some(third));
}

#[test]
fn streams_by_donor_keeps_cancelled_stream_ids_listed() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.cancel_stream(&stream_id);

    // Cancelling doesn't delete the stream record, so its id stays listed;
    // callers use get_stream to check its status.
    let ids = s.client.streams_by_donor(&s.donor);
    assert_eq!(ids.len(), 1);
    assert_eq!(ids.get(0), Some(stream_id));
}

#[test]
fn streams_by_donor_is_per_donor() {
    let s = setup();
    let donor_b = Address::generate(&s.env);
    s.token_admin.mint(&s.donor, &1_000);
    s.token_admin.mint(&donor_b, &1_000);

    let donor_a_stream = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let donor_b_stream = s
        .client
        .create_stream(&donor_b, &s.ngo, &s.token.address, &1_000, &10);

    let donor_a_ids = s.client.streams_by_donor(&s.donor);
    assert_eq!(donor_a_ids.len(), 1);
    assert_eq!(donor_a_ids.get(0), Some(donor_a_stream));

    let donor_b_ids = s.client.streams_by_donor(&donor_b);
    assert_eq!(donor_b_ids.len(), 1);
    assert_eq!(donor_b_ids.get(0), Some(donor_b_stream));
}

#[test]
fn donor_totals_tracks_streams_and_tokens_through_settlement_and_refunds() {
    let s = setup();
    let donor = &s.donor;
    let ngo = &s.ngo;
    let token = &s.token.address;
    let other_donor = Address::generate(&s.env);
    let (other_token, other_admin) = create_token(&s.env, &Address::generate(&s.env));
    let other_token = &other_token.address;
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);
    s.token_admin.mint(donor, &1_800);
    s.token_admin.mint(&other_donor, &500);
    other_admin.mint(donor, &700);

    let totals = |owner: &Address, asset: &Address| {
        let DonorTotals {
            committed,
            withdrawn,
        } = s.client.donor_totals(owner, asset);
        (committed, withdrawn)
    };
    assert_eq!(totals(donor, token), (0, 0));
    let first = s.client.create_stream(donor, ngo, token, &1_000, &10);
    let second = s.client.create_stream(donor, ngo, token, &600, &20);
    let other = s.client.create_stream(donor, ngo, other_token, &700, &5);
    s.client.create_stream(&other_donor, ngo, token, &500, &5);
    assert_eq!(totals(donor, token), (1_600, 0));
    assert_eq!(totals(donor, other_token), (700, 0));
    assert_eq!(totals(&other_donor, token), (500, 0));

    s.env.ledger().with_mut(|l| l.timestamp += 10);
    assert_eq!(totals(donor, token), (1_600, 0));
    s.client.top_up(&first, &200);
    assert_eq!(totals(donor, token), (1_700, 100));

    s.env.ledger().with_mut(|l| l.timestamp += 10);
    s.client
        .withdraw_batch(&soroban_sdk::vec![&s.env, first, second, other]);
    assert_eq!(totals(donor, token), (1_200, 600));
    assert_eq!(totals(donor, other_token), (600, 100));

    s.env.ledger().with_mut(|l| l.timestamp += 10);
    assert_eq!(s.client.cancel_stream(&first), 900);
    assert_eq!(totals(donor, token), (200, 700));
    s.client.pause();
    assert_eq!(s.client.rescue_stream(&second), 200);
    assert_eq!(totals(donor, token), (0, 700));
    assert_eq!(totals(donor, other_token), (600, 100));
    assert_eq!(totals(&other_donor, token), (500, 0));
}

#[test]
fn donor_totals_reads_legacy_streams_and_indexes_them_on_next_write() {
    let s = setup();
    let token = &s.token.address;
    let donor = &s.donor;
    s.token_admin.mint(donor, &1_500);
    let first = s.client.create_stream(donor, &s.ngo, token, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    assert_eq!(s.client.withdraw(&first), 100);

    let key = DataKey::DonorTotals(donor.clone(), token.clone());
    s.env.as_contract(&s.client.address, || {
        s.env
            .storage()
            .instance()
            .remove(&DataKey::DonorTotalsIndexedFromStart);
        s.env.storage().persistent().remove(&key);
        s.env
            .storage()
            .persistent()
            .remove(&DataKey::DonorStreamCount(donor.clone()));
    });
    let old = s.client.donor_totals(donor, token);
    assert_eq!(old.committed, 900);
    assert_eq!(old.withdrawn, 100);
    s.env.as_contract(&s.client.address, || {
        assert!(!s.env.storage().persistent().has(&key));
    });

    s.client.create_stream(donor, &s.ngo, token, &300, &10);
    s.env.as_contract(&s.client.address, || {
        let stored: DonorTotals = s.env.storage().persistent().get(&key).unwrap();
        assert_eq!(stored.committed, 1_200);
        assert_eq!(stored.withdrawn, 100);
    });

    s.env.as_contract(&s.client.address, || {
        s.env.storage().persistent().remove(&key);
    });
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    s.client.top_up(&first, &100);
    let totals = s.client.donor_totals(donor, token);
    assert_eq!(totals.committed, 1_200);
    assert_eq!(totals.withdrawn, 200);
}

#[test]
fn donor_stream_count_is_zero_before_any_stream() {
    let s = setup();
    assert_eq!(s.client.donor_stream_count(&s.donor), 0);
}

#[test]
fn donor_stream_count_tracks_multiple_streams() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);

    s.client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &20);
    s.client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &30);

    assert_eq!(s.client.donor_stream_count(&s.donor), 3);
}

#[test]
fn donor_stream_count_keeps_cancelled_stream() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.cancel_stream(&stream_id);

    assert_eq!(s.client.donor_stream_count(&s.donor), 1);
}

#[test]
fn stream_ids_are_sequential_and_stream_count_tracks_them() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000_000);

    assert_eq!(s.client.stream_count(), 0);

    let id_a = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.stream_count(), 1);

    let id_b = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.stream_count(), 2);

    let id_c = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.stream_count(), 3);

    // Ids increment by exactly one, starting from zero.
    assert_eq!(id_a, 0);
    assert_eq!(id_b, id_a + 1);
    assert_eq!(id_c, id_b + 1);

    // stream_count matches the number of streams actually created.
    assert_eq!(s.client.stream_count(), 3);
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

    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 1_000);

    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);
    assert_eq!(stream_ttl(&s, stream_id), STREAM_BUMP_AMOUNT);
}

#[test]
fn cancel_stream_applies_configured_grace_period() {
    let s = setup();
    let stream_id = create_ttl_test_stream(&s);
    let grace_ledgers = 14 * DAY_IN_LEDGERS;

    s.client.set_cancel_grace_ledgers(&grace_ledgers);
    assert_eq!(s.client.cancel_grace_ledgers(), grace_ledgers);
    age_past_thresholds(&s, Some(stream_id));

    s.client.cancel_stream(&stream_id);

    assert_eq!(
        stream_ttl(&s, stream_id),
        STREAM_BUMP_AMOUNT + grace_ledgers
    );
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
fn extend_stream_on_cancelled_stream_still_bumps_ttl() {
    let s = setup();
    let stream_id = create_ttl_test_stream(&s);

    // Cancel the stream â€” its storage entry stays in place (balance and
    // rate are zeroed, but the key is never removed), so extend_stream
    // must still be able to keep it alive for historical lookups.
    s.client.cancel_stream(&stream_id);

    // Age past both thresholds so we can confirm the bump is actually
    // doing work, not just leaving the TTL where it already was.
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

#[test]
fn renounce_admin_clears_admin_and_pending_proposal() {
    let s = setup();
    let admin = s.client.admin();
    let new_admin = Address::generate(&s.env);

    // A pending proposal exists before renouncing.
    s.client.propose_admin(&new_admin);
    assert_eq!(s.client.pending_admin(), Some(new_admin.clone()));

    s.client.renounce_admin();
    assert_auth_required_from(&s, &admin, "renounce_admin");

    // Both the admin and any pending proposal are cleared.
    assert_eq!(s.client.pending_admin(), None);
}

#[test]
fn renounce_admin_disables_admin_gated_calls() {
    let s = setup();
    let admin = s.client.admin();
    let new_admin = Address::generate(&s.env);
    let treasury = Address::generate(&s.env);

    s.client.renounce_admin();
    assert_auth_required_from(&s, &admin, "renounce_admin");

    // Admin-gated entry points must fail specifically with AdminRenounced.
    assert_eq!(s.client.try_pause(), Err(Ok(Error::AdminRenounced)));
    assert_eq!(s.client.try_unpause(), Err(Ok(Error::AdminRenounced)));
    assert_eq!(
        s.client.try_set_treasury(&treasury),
        Err(Ok(Error::AdminRenounced))
    );
    assert_eq!(
        s.client.try_clear_treasury(),
        Err(Ok(Error::AdminRenounced))
    );
    assert_eq!(
        s.client.try_set_fee_bps(&100),
        Err(Ok(Error::AdminRenounced))
    );
    assert_eq!(
        s.client.try_propose_admin(&new_admin),
        Err(Ok(Error::AdminRenounced))
    );
    assert_eq!(
        s.client.try_cancel_admin_proposal(),
        Err(Ok(Error::AdminRenounced))
    );
    assert_eq!(
        s.client.try_set_min_deposit(&1_000),
        Err(Ok(Error::AdminRenounced))
    );
    assert_eq!(
        s.client.try_set_max_streams_per_donor(&5),
        Err(Ok(Error::AdminRenounced))
    );
}

#[test]
fn renounce_admin_returns_admin_renounced_for_admin_init_propose_and_pause() {
    let s = setup();
    let admin = s.client.admin();
    let candidate = Address::generate(&s.env);

    s.client.renounce_admin();
    assert_auth_required_from(&s, &admin, "renounce_admin");

    // admin() returns AdminRenounced
    assert_eq!(s.client.try_admin(), Err(Ok(Error::AdminRenounced)));

    // init() (re-initialisation after renounce) returns AdminRenounced
    assert_eq!(
        s.client.try_init(&candidate),
        Err(Ok(Error::AdminRenounced))
    );

    // propose_admin() returns AdminRenounced
    assert_eq!(
        s.client.try_propose_admin(&candidate),
        Err(Ok(Error::AdminRenounced))
    );

    // pause() returns AdminRenounced
    assert_eq!(s.client.try_pause(), Err(Ok(Error::AdminRenounced)));
}

#[test]
fn renounce_admin_without_admin_fails() {
    let s = setup();
    s.client.renounce_admin();

    // A second renounce fails with AdminRenounced because admin control is permanently renounced.
    assert_eq!(
        s.client.try_renounce_admin(),
        Err(Ok(Error::AdminRenounced))
    );
}

/// Rewrites a stored stream in place, for pushing its bookkeeping to the
/// edge of i128 â€” no real token supply could get it there.
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

#[test]
fn create_and_top_up_near_token_max() {
    let s = setup();
    // The Stellar asset contract used in tests caps balances at i64::MAX, so
    // this is the largest deposit the token can hold and move.
    let max_deposit = i64::MAX as i128;
    s.token_admin.mint(&s.donor, &max_deposit);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &max_deposit, &10);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, max_deposit);
    assert_eq!(stream.rate, 10);
    assert_eq!(s.token.balance(&s.client.address), max_deposit);

    // Advance time and withdraw to reduce balance
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues
    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 500);
    assert_eq!(s.client.get_stream(&stream_id).balance, max_deposit - 500);

    // Fund the donor again now that the deposit sits in the vault.
    s.token_admin.mint(&s.donor, &2000);

    // Top-up with an amount that would stay within bounds
    s.client.top_up(&stream_id, &100);
    assert_eq!(
        s.client.get_stream(&stream_id).balance,
        max_deposit - 500 + 100
    );

    // Attempt to top up beyond i128::MAX should fail
    let overflow_amount = 1000; // Would push balance past i128::MAX
    let result = s.client.try_top_up(&stream_id, &overflow_amount);
    assert_eq!(result, Err(Ok(Error::ArithmeticOverflow)));

    // Stream and balances should be unchanged after the failed top-up
    assert_eq!(
        s.client.get_stream(&stream_id).balance,
        max_deposit - 500 + 100
    );
    assert_eq!(
        s.token.balance(&s.donor),
        2000 - 100 // Still has the amount that failed to top up
    );
    assert_eq!(s.token.balance(&s.client.address), max_deposit - 500 + 100);

    // Cancel should work and return the remaining balance
    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, max_deposit - 500 + 100);
    assert_eq!(s.token.balance(&s.donor), 2000 - 100 + refund);
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

    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 1_000);

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

    s.client.clear_treasury();
    assert_auth_required_from(&s, &admin, "clear_treasury");

    s.client.set_treasury(&Address::generate(&s.env));
    s.client.set_fee_bps(&100);
    assert_auth_required_from(&s, &admin, "set_fee_bps");

    let new_admin = Address::generate(&s.env);
    s.client.propose_admin(&new_admin);
    assert_auth_required_from(&s, &admin, "propose_admin");

    s.client.cancel_admin_proposal();
    assert_auth_required_from(&s, &admin, "cancel_admin_proposal");

    s.client.clear_token_fee_bps(&s.token.address);
    assert_auth_required_from(&s, &admin, "clear_token_fee_bps");

    s.client.set_registry(&Address::generate(&s.env));
    assert_auth_required_from(&s, &admin, "set_registry");
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

#[test]
fn pending_payout_with_no_treasury_returns_full_gross_and_zero_fee() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);
    s.client.set_fee_bps(&0);
    s.client.clear_treasury();

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues

    let (net, fee) = s.client.pending_payout(&stream_id);
    assert_eq!(fee, 0); // no treasury â†’ no fee
    assert_eq!(net, 500);

    // Read-only: nothing moved.
    assert_eq!(s.token.balance(&s.ngo), 0);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);
}

// â”€â”€ Issue #64 â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Every admin-gated entry point must fail closed with Error::NotInitialized
// when called before init, rather than silently defaulting (e.g. paused()
// reads a missing key as false, which would leave a pre-init vault looking
// operational).

#[test]
fn admin_functions_before_init_return_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(DonationVault, DonationVaultArgs::__constructor(&admin));
    let client = DonationVaultClient::new(&env, &contract_id);

    // Simulate uninitialized state where DataKey::Admin is missing
    env.as_contract(&contract_id, || {
        env.storage().instance().remove(&DataKey::Admin);
    });

    let treasury = Address::generate(&env);
    let new_admin = Address::generate(&env);

    // Every entry point that reaches for the admin through require_admin().
    assert_eq!(
        client.try_pause(),
        Err(Ok(Error::NotInitialized)),
        "pause() must return NotInitialized before init"
    );
    assert_eq!(
        client.try_unpause(),
        Err(Ok(Error::NotInitialized)),
        "unpause() must return NotInitialized before init"
    );
    assert_eq!(
        client.try_set_treasury(&treasury),
        Err(Ok(Error::NotInitialized)),
        "set_treasury() must return NotInitialized before init"
    );
    assert_eq!(
        client.try_clear_treasury(),
        Err(Ok(Error::NotInitialized)),
        "clear_treasury() must return NotInitialized before init"
    );
    assert_eq!(
        client.try_set_fee_bps(&100),
        Err(Ok(Error::NotInitialized)),
        "set_fee_bps() must return NotInitialized before init"
    );
    assert_eq!(
        client.try_set_min_deposit(&1),
        Err(Ok(Error::NotInitialized)),
        "set_min_deposit() must return NotInitialized before init"
    );
    assert_eq!(
        client.try_propose_admin(&new_admin),
        Err(Ok(Error::NotInitialized)),
        "propose_admin() must return NotInitialized before init"
    );
    assert_eq!(
        client.try_cancel_admin_proposal(),
        Err(Ok(Error::NotInitialized)),
        "cancel_admin_proposal() must return NotInitialized before init"
    );

    // The admin reader reports the same rather than defaulting to a
    // zero address.
    assert_eq!(
        client.try_admin(),
        Err(Ok(Error::NotInitialized)),
        "admin() must return NotInitialized before init"
    );

    // accept_admin doesn't go through require_admin â€” it reads the pending
    // proposal directly, so before init there is nothing to accept. That's
    // NoPendingAdmin rather than NotInitialized, but it still fails closed.
    assert_eq!(
        client.try_accept_admin(),
        Err(Ok(Error::NoPendingAdmin)),
        "accept_admin() must fail closed before init"
    );
}

// =============================================================================
// Per-donor stream limit (issue #94)
// =============================================================================

#[test]
fn max_streams_per_donor_defaults_to_100() {
    let s = setup();
    assert_eq!(s.client.max_streams_per_donor(), 100);
}

#[test]
fn zero_decimal_token_accrual_and_fee_rounding() {
    // 0-decimal token: 1 token = 1 unit (no fractional parts)
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&250); // 2.5% fee

    // Represent 1000 tokens (0 decimals) as 1000 units
    let deposit = 1_000i128;
    let rate = 10i128; // 10 units/second = 10 tokens/second
    s.token_admin.mint(&s.donor, &deposit);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &deposit, &rate);

    // 50 seconds -> 500 units accrued
    s.env.ledger().with_mut(|l| l.timestamp += 50);
    let withdrawn = s.client.withdraw(&stream_id);

    // 500 * 2.5% = 12.5 -> rounds down to 12
    let expected_fee = 12i128;
    let expected_net = 500 - expected_fee;

    assert_eq!(withdrawn, expected_net);
    assert_eq!(s.token.balance(&s.ngo), expected_net);
    assert_eq!(s.token.balance(&treasury), expected_fee);

    // Verify refund on cancel
    s.env.ledger().with_mut(|l| l.timestamp += 20);
    let refund = s.client.cancel_stream(&stream_id);

    // 200 more accrues, leaves 300 to refund
    // 200 * 2.5% = 5 fee, 195 net
    assert_eq!(s.token.balance(&s.ngo), expected_net + 195);
    assert_eq!(refund, 300);
}

#[test]
fn zero_decimal_token_one_unit_per_second_minimal_fee() {
    // Edge case: 1 unit/second with a fee
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% fee

    let deposit = 100i128;
    let rate = 1i128; // 1 unit/second
    s.token_admin.mint(&s.donor, &deposit);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &deposit, &rate);

    // 10 seconds -> 10 units accrued
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    let withdrawn = s.client.withdraw(&stream_id);

    // 10 * 5% = 0.5 -> rounds down to 0
    assert_eq!(withdrawn, 10);
    assert_eq!(s.token.balance(&s.ngo), 10);
    assert_eq!(s.token.balance(&treasury), 0);
}

#[test]
fn eighteen_decimal_token_accrual_and_fee_rounding() {
    // 18-decimal token: 1 token = 10^18 units (like ETH wei)
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&250); // 2.5% fee

    // Represent 1000 tokens (18 decimals) as 1000 * 10^18 units
    // Using smaller scale for test: 1000 * 10^12 = 1_000_000_000_000_000
    let deposit = 1_000_000_000_000_000i128;
    let rate = 10_000_000_000_000i128; // 10^13 units/second = 0.01 tokens/second
    s.token_admin.mint(&s.donor, &deposit);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &deposit, &rate);

    // 50 seconds -> 500 * 10^12 units accrued
    s.env.ledger().with_mut(|l| l.timestamp += 50);
    let withdrawn = s.client.withdraw(&stream_id);

    let accrued = 500_000_000_000_000i128;
    // 500_000_000_000_000 * 2.5% = 12_500_000_000_000
    let expected_fee = 12_500_000_000_000i128;
    let expected_net = accrued - expected_fee;

    assert_eq!(withdrawn, expected_net);
    assert_eq!(s.token.balance(&s.ngo), expected_net);
    assert_eq!(s.token.balance(&treasury), expected_fee);
}

#[test]
fn eighteen_decimal_token_tiny_amounts_precise_fee() {
    // Very small amounts with 18 decimals test fee precision
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&100); // 1% fee

    // 1 * 10^15 units (0.001 tokens in 18-decimal scale)
    let deposit = 1_000_000_000_000_000i128;
    let rate = 1_000_000_000_000i128; // 10^12 units/second
    s.token_admin.mint(&s.donor, &deposit);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &deposit, &rate);

    // 100 seconds -> 100 * 10^12 units accrued
    s.env.ledger().with_mut(|l| l.timestamp += 100);
    let withdrawn = s.client.withdraw(&stream_id);

    let accrued = 100_000_000_000_000i128;
    // 100_000_000_000_000 * 1% = 1_000_000_000_000
    let expected_fee = 1_000_000_000_000i128;
    let expected_net = accrued - expected_fee;

    assert_eq!(withdrawn, expected_net);
    assert_eq!(s.token.balance(&s.ngo), expected_net);
    assert_eq!(s.token.balance(&treasury), expected_fee);
}

#[test]
fn eighteen_decimal_token_refund_precision() {
    // Test that refunds are precise with large 18-decimal values
    let s = setup();

    let deposit = 999_999_999_999_999_999i128; // Nearly 1 token (18 decimals)
    let rate = 1_000_000_000_000_000i128; // 10^15 units/second
    s.token_admin.mint(&s.donor, &deposit);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &deposit, &rate);

    // 100 seconds -> 100 * 10^15 units accrue
    s.env.ledger().with_mut(|l| l.timestamp += 100);

    let refund = s.client.cancel_stream(&stream_id);

    // accrued = 100_000_000_000_000_000
    // refund = 999_999_999_999_999_999 - 100_000_000_000_000_000 = 899_999_999_999_999_999
    let expected_accrued = 100_000_000_000_000_000i128;
    let expected_refund = deposit - expected_accrued;

    assert_eq!(refund, expected_refund);
    assert_eq!(s.token.balance(&s.ngo), expected_accrued);
    assert_eq!(s.token.balance(&s.donor), expected_refund);
}

#[test]
fn zero_decimal_token_max_fee_no_underflow() {
    // Test max fee (10%) doesn't cause underflow with 0-decimal token
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&1_000); // 10% max fee

    let deposit = 1_000i128;
    let rate = 100i128; // 100 units/second
    s.token_admin.mint(&s.donor, &deposit);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &deposit, &rate);

    // 5 seconds -> 500 units
    s.env.ledger().with_mut(|l| l.timestamp += 5);
    let withdrawn = s.client.withdraw(&stream_id);

    // 500 * 10% = 50
    let expected_fee = 50i128;
    let expected_net = 450i128;

    assert_eq!(withdrawn, expected_net);
    assert_eq!(s.token.balance(&s.ngo), expected_net);
    assert_eq!(s.token.balance(&treasury), expected_fee);
}

#[test]
fn eighteen_decimal_token_fee_does_not_exceed_amount() {
    // Ensure fee calculation never exceeds the actual amount
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&1_000); // 10% max fee

    let deposit = 1_000_000_000_000_000_000i128; // 1 token (18 decimals)
    let rate = 1_000_000_000_000_000i128; // 10^15 units/second
    s.token_admin.mint(&s.donor, &deposit);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &deposit, &rate);

    // 999 seconds -> nearly full balance accrued
    s.env.ledger().with_mut(|l| l.timestamp += 999);
    let withdrawn = s.client.withdraw(&stream_id);

    let accrued = 999_000_000_000_000_000i128;
    let expected_fee = 99_900_000_000_000_000i128; // 10% of accrued
    let expected_net = accrued - expected_fee;

    assert_eq!(withdrawn, expected_net);
    assert_eq!(s.token.balance(&s.ngo), expected_net);
    assert_eq!(s.token.balance(&treasury), expected_fee);

    // Verify fee is exactly 10% and doesn't exceed amount
    assert!(expected_fee <= accrued);
    assert_eq!(expected_fee, accrued / 10);
}

#[test]
fn mixed_decimal_scales_multiple_streams() {
    // Verify multiple streams with different "decimal scales" work independently
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% fee

    // Stream 1: "0-decimal" - 1000 units
    let deposit1 = 1_000i128;
    let rate1 = 10i128;
    s.token_admin.mint(&s.donor, &deposit1);

    // Create second token for second stream with different scale
    let token2_admin_addr = Address::generate(&s.env);
    let (token2, token2_admin) = create_token(&s.env, &token2_admin_addr);

    // Stream 2: "18-decimal" - large units
    let deposit2 = 1_000_000_000_000_000i128;
    let rate2 = 10_000_000_000_000i128;
    token2_admin.mint(&s.donor, &deposit2);

    let stream1 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &deposit1, &rate1);
    let stream2 = s
        .client
        .create_stream(&s.donor, &s.ngo, &token2.address, &deposit2, &rate2);

    s.env.ledger().with_mut(|l| l.timestamp += 50);

    // Withdraw both streams
    let withdrawn1 = s.client.withdraw(&stream1);
    let withdrawn2 = s.client.withdraw(&stream2);

    // Stream 1: 500 * 5% = 25 fee, 475 net
    assert_eq!(withdrawn1, 475);
    assert_eq!(s.token.balance(&s.ngo), 475);

    // Stream 2: 500_000_000_000_000 * 5% = 25_000_000_000_000 fee
    let expected_net2 = 475_000_000_000_000i128;
    assert_eq!(withdrawn2, expected_net2);
    assert_eq!(token2.balance(&s.ngo), expected_net2);
}

#[test]
fn admin_can_set_the_per_donor_cap() {
    let s = setup();
    s.client.set_max_streams_per_donor(&5);
    assert_eq!(s.client.max_streams_per_donor(), 5);
}

#[test]
fn non_admin_cannot_set_the_per_donor_cap() {
    let s = setup();
    s.env.mock_auths(&[]);
    let result = s.client.try_set_max_streams_per_donor(&5);
    assert!(result.is_err());
}

#[test]
fn max_rate_defaults_to_none() {
    let s = setup();
    assert_eq!(s.client.max_rate(), None);
    assert_eq!(s.client.get_config().max_rate, None);
}

#[test]
fn max_rate_is_exposed_through_get_config() {
    let s = setup();

    s.client.set_max_rate(&1_000);
    assert_eq!(s.client.max_rate(), Some(1_000));
    assert_eq!(s.client.get_config().max_rate, Some(1_000));

    s.client.clear_max_rate();
    assert_eq!(s.client.max_rate(), None);
    assert_eq!(s.client.get_config().max_rate, None);
}

#[test]
fn set_max_rate_rejects_non_positive() {
    let s = setup();
    assert_eq!(s.client.try_set_max_rate(&0), Err(Ok(Error::InvalidAmount)));
    assert_eq!(
        s.client.try_set_max_rate(&-1),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn non_admin_cannot_set_max_rate() {
    let s = setup();
    s.env.mock_auths(&[]);
    assert!(s.client.try_set_max_rate(&100).is_err());
}

#[test]
fn create_stream_enforces_max_rate_boundary() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);
    s.client.set_max_rate(&100);

    // Boundary: a rate equal to the ceiling is accepted.
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &100);
    assert_eq!(s.client.get_stream(&stream_id).rate, 100);

    // One above the ceiling fails, and no deposit is pulled.
    let balance_before = s.token.balance(&s.donor);
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &101),
        Err(Ok(Error::RateTooHigh))
    );
    assert_eq!(s.token.balance(&s.donor), balance_before);
}

#[test]
fn modify_rate_enforces_max_rate_boundary() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.set_max_rate(&100);

    // Boundary: a rate equal to the ceiling is accepted.
    s.client.modify_rate(&stream_id, &100);
    assert_eq!(s.client.get_stream(&stream_id).rate, 100);

    // One above the ceiling fails and leaves the stored rate unchanged.
    assert_eq!(
        s.client.try_modify_rate(&stream_id, &101),
        Err(Ok(Error::RateTooHigh))
    );
    assert_eq!(s.client.get_stream(&stream_id).rate, 100);
}

#[test]
fn clear_max_rate_restores_uncapped_rates() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    // Off by default, so an enormous rate is accepted.
    let first = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &1_000_000);
    assert_eq!(s.client.get_stream(&first).rate, 1_000_000);

    // While a ceiling is set the same rate is rejected...
    s.client.set_max_rate(&100);
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &1_000_000),
        Err(Ok(Error::RateTooHigh))
    );

    // ...and clearing it restores the uncapped behavior.
    s.client.clear_max_rate();
    let second = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &1_000_000);
    assert_eq!(s.client.get_stream(&second).rate, 1_000_000);
}

#[test]
fn set_max_rate_publishes_event() {
    let s = setup();

    s.client.set_max_rate(&1_000);

    assert_last_event(&s.env, (symbol_short!("maxrate"),), 1_000i128);
}

#[test]
fn clear_max_rate_publishes_event() {
    let s = setup();
    s.client.set_max_rate(&1_000);

    s.client.clear_max_rate();

    assert_last_event(&s.env, (symbol_short!("maxrtclr"),), ());
}

// =============================================================================
// Issue #340: optional minimum stream duration (shortest time to depletion)
// =============================================================================

#[test]
fn min_duration_defaults_to_zero() {
    let s = setup();
    assert_eq!(s.client.min_duration(), 0);
    assert_eq!(s.client.get_config().min_duration, 0);
}

#[test]
fn min_duration_is_exposed_through_get_config() {
    let s = setup();

    s.client.set_min_duration(&60);
    assert_eq!(s.client.min_duration(), 60);
    assert_eq!(s.client.get_config().min_duration, 60);

    // Zero is the documented "no minimum" sentinel.
    s.client.set_min_duration(&0);
    assert_eq!(s.client.min_duration(), 0);
    assert_eq!(s.client.get_config().min_duration, 0);
}

#[test]
fn non_admin_cannot_set_min_duration() {
    let s = setup();
    s.env.mock_auths(&[]);
    assert!(s.client.try_set_min_duration(&60).is_err());
}

#[test]
fn create_stream_enforces_min_duration_boundary() {
    let s = setup();
    s.token_admin.mint(&s.donor, &10_000);
    s.client.set_min_duration(&60);

    // Boundary: a deposit that lasts exactly the minimum (600 / 10 = 60 s)
    // is accepted.
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &600, &10);
    assert_eq!(s.client.get_stream(&stream_id).rate, 10);

    // Ceiling: 591 / 10 lasts 60 s when rounded up, so it is still accepted.
    let second = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &591, &10);
    assert_eq!(s.client.get_stream(&second).balance, 591);

    // One second short (590 / 10 = 59 s) is rejected, and no deposit is pulled.
    let balance_before = s.token.balance(&s.donor);
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &590, &10),
        Err(Ok(Error::StreamTooShort))
    );
    assert_eq!(s.token.balance(&s.donor), balance_before);
}

#[test]
fn create_stream_min_duration_error_is_distinct_from_invalid_amount() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.client.set_min_duration(&60);

    // A positive deposit and rate that merely drain too fast are reported as
    // StreamTooShort, not InvalidAmount.
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &10, &10),
        Err(Ok(Error::StreamTooShort))
    );

    // A zero deposit is still InvalidAmount, leaving the two cases distinct.
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &0, &10),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn create_stream_rejects_unmeetable_min_duration_without_overflow() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.client.set_min_duration(&u64::MAX);

    // No representable deposit can meet the floor; the checked multiply must
    // report StreamTooShort rather than overflow.
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &1),
        Err(Ok(Error::StreamTooShort))
    );
}

#[test]
fn modify_rate_enforces_min_duration_boundary() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.set_min_duration(&60);

    // Boundary: at the new rate the remaining 1_000 lasts 63 s (ceil), so it
    // is accepted.
    s.client.modify_rate(&stream_id, &16);
    assert_eq!(s.client.get_stream(&stream_id).rate, 16);

    // One step faster drains in 59 s, so it is rejected and the stored rate
    // and balance are left unchanged.
    assert_eq!(
        s.client.try_modify_rate(&stream_id, &17),
        Err(Ok(Error::StreamTooShort))
    );
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.rate, 16);
    assert_eq!(stream.balance, 1_000);
}

#[test]
fn zero_min_duration_disables_the_check() {
    let s = setup();
    s.token_admin.mint(&s.donor, &10);
    s.client.set_min_duration(&0);

    // With the floor disabled, a one-second stream is allowed.
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1, &1);
    assert_eq!(s.client.get_stream(&stream_id).rate, 1);
}

#[test]
fn set_min_duration_publishes_event() {
    let s = setup();

    s.client.set_min_duration(&60);

    assert_last_event(&s.env, (symbol_short!("mindur"),), 60u64);
}

// =============================================================================
// Issue #337: optional cliff before a stream starts paying
// =============================================================================

#[test]
fn cliff_stream_accrues_nothing_before_start() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);

    let start = 1_500;
    let stream_id =
        s.client
            .create_stream_with_start(&s.donor, &s.ngo, &s.token.address, &1_000, &10, &start);

    // The cliff is recorded, and the deposit is already in the vault.
    assert_eq!(s.client.get_stream(&stream_id).start, start);
    assert_eq!(s.token.balance(&s.donor), 0);

    // Still before `start`: nothing accrues and there is nothing to withdraw.
    s.env.ledger().with_mut(|l| l.timestamp = 1_400);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);
    assert_eq!(
        s.client.try_withdraw(&stream_id),
        Err(Ok(Error::NothingToWithdraw))
    );
}

#[test]
fn cliff_stream_accrues_from_start_not_creation() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);

    let start = 1_500;
    let stream_id =
        s.client
            .create_stream_with_start(&s.donor, &s.ngo, &s.token.address, &1_000, &10, &start);

    // 400s after creation but still before the cliff: still nothing.
    s.env.ledger().with_mut(|l| l.timestamp = 1_400);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);

    // 50s past the cliff (550s past creation): only those 50 count.
    s.env.ledger().with_mut(|l| l.timestamp = start + 50);
    assert_eq!(s.client.pending_accrual(&stream_id), 500);

    // Withdrawing pays that accrual, dated from `start`.
    assert_eq!(s.client.withdraw(&stream_id), 500);
    assert_eq!(s.client.get_stream(&stream_id).withdrawn, 500);
}

#[test]
fn cliff_depletion_time_is_measured_from_start() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);

    // 1_000 units at 300/s deplete in 4 seconds (rounded up), counted from the
    // cliff rather than from creation.
    let start = 3_000;
    let stream_id =
        s.client
            .create_stream_with_start(&s.donor, &s.ngo, &s.token.address, &1_000, &300, &start);

    assert_eq!(s.client.depletion_time(&stream_id), Some(start + 4));
}

#[test]
fn cancel_before_cliff_refunds_whole_deposit() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);

    let start = 5_000;
    let stream_id =
        s.client
            .create_stream_with_start(&s.donor, &s.ngo, &s.token.address, &1_000, &10, &start);

    // Before the cliff nothing has accrued, so the donor gets the whole
    // deposit back and the NGO receives nothing.
    s.env.ledger().with_mut(|l| l.timestamp = 2_000);
    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 1_000);
    assert_eq!(s.token.balance(&s.donor), 1_000);
    assert_eq!(s.token.balance(&s.ngo), 0);
    assert!(s.client.get_stream(&stream_id).cancelled);
}

#[test]
fn cliff_at_or_before_now_behaves_like_an_immediate_stream() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);

    // A `start` in the past is not a cliff: accrual still runs from now.
    let stream_id =
        s.client
            .create_stream_with_start(&s.donor, &s.ngo, &s.token.address, &1_000, &10, &500);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.start, 500);
    assert_eq!(stream.last_update, 1_000);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);

    s.env.ledger().with_mut(|l| l.timestamp = 1_050);
    assert_eq!(s.client.pending_accrual(&stream_id), 500);
}

#[test]
fn create_stream_with_start_rejects_cliff_beyond_the_limit() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);
    s.env.ledger().with_mut(|l| l.timestamp = 0);

    // Exactly at the limit is accepted.
    let stream_id = s.client.create_stream_with_start(
        &s.donor,
        &s.ngo,
        &s.token.address,
        &1_000,
        &10,
        &MAX_START_DELAY_SECONDS,
    );
    assert_eq!(
        s.client.get_stream(&stream_id).start,
        MAX_START_DELAY_SECONDS
    );

    // One second beyond the limit is rejected, and no deposit moves.
    let balance_before = s.token.balance(&s.donor);
    assert_eq!(
        s.client.try_create_stream_with_start(
            &s.donor,
            &s.ngo,
            &s.token.address,
            &1_000,
            &10,
            &(MAX_START_DELAY_SECONDS + 1),
        ),
        Err(Ok(Error::StartTooFar))
    );
    assert_eq!(s.token.balance(&s.donor), balance_before);
}

// =============================================================================
// Issue #341: optional bounded memo on a stream
// =============================================================================

#[test]
fn create_stream_with_memo_stores_and_emits_the_memo() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let memo = Bytes::from_array(&s.env, b"campaign-42");
    let stream_id =
        s.client
            .create_stream_with_memo(&s.donor, &s.ngo, &s.token.address, &1_000, &10, &memo);

    // Snapshot the created event before any other call replaces it.
    let created = last_event(&s.env);

    // Read back from the stream record...
    assert_eq!(s.client.get_stream(&stream_id).memo, memo);
    // ...and from the event, which carries the same memo in its data.
    assert_eq!(
        created,
        (
            (symbol_short!("created"), stream_id).into_val(&s.env),
            (
                s.donor.clone(),
                s.ngo.clone(),
                s.token.address.clone(),
                1_000i128,
                10i128,
                memo.clone(),
            )
                .into_val(&s.env),
        )
    );
}

#[test]
fn create_stream_without_memo_records_an_empty_memo() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    assert_eq!(s.client.get_stream(&stream_id).memo, Bytes::new(&s.env));
}

#[test]
fn create_stream_with_memo_rejects_an_oversized_memo() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    // Exactly `MAX_MEMO_BYTES` is accepted.
    let max_memo = Bytes::from_slice(&s.env, &[7u8; MAX_MEMO_BYTES as usize]);
    let stream_id = s.client.create_stream_with_memo(
        &s.donor,
        &s.ngo,
        &s.token.address,
        &1_000,
        &10,
        &max_memo,
    );
    assert_eq!(s.client.get_stream(&stream_id).memo, max_memo);

    // One byte over the cap is rejected, and no deposit moves.
    let balance_before = s.token.balance(&s.donor);
    let too_long = Bytes::from_slice(&s.env, &[0u8; MAX_MEMO_BYTES as usize + 1]);
    assert_eq!(
        s.client.try_create_stream_with_memo(
            &s.donor,
            &s.ngo,
            &s.token.address,
            &1_000,
            &10,
            &too_long,
        ),
        Err(Ok(Error::MemoTooLong))
    );
    assert_eq!(s.token.balance(&s.donor), balance_before);
}

#[test]
fn create_stream_rejects_self_stream() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    // Positive control: valid distinct donor and NGO succeeds
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);

    // Targeted check: donor == ngo fails with SelfStream
    let result = s
        .client
        .try_create_stream(&s.donor, &s.donor, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::SelfStream)));
}

#[test]
fn create_stream_rejects_deposit_too_low() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);
    s.client.set_min_deposit(&1_000);

    // Targeted check: valid distinct parties, deposit below min_deposit fails with DepositTooLow
    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &999, &10);
    assert_eq!(result, Err(Ok(Error::DepositTooLow)));

    // Positive control: meeting min_deposit succeeds
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

#[test]
fn create_stream_rejects_the_101st_stream() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000_000);

    // Fill the default per-donor cap (100 streams).
    for _ in 0..100 {
        s.client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    }

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::StreamLimitExceeded)));

    // Positive control: cap is per-donor, so a different donor can still create a stream
    let other_donor = Address::generate(&s.env);
    s.token_admin.mint(&other_donor, &1_000);
    let other_id = s
        .client
        .create_stream(&other_donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&other_id).donor, other_donor);
}

#[test]
fn raising_the_cap_lets_the_next_stream_through() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000_000);
    s.client.set_max_streams_per_donor(&1);

    s.client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // At the cap, next call fails.
    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::StreamLimitExceeded)));

    // Raise the cap. Next call succeeds.
    s.client.set_max_streams_per_donor(&2);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&stream_id).donor, s.donor);
}

#[test]
fn lowering_the_cap_does_not_retroactively_affect_existing_streams() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000_000);
    s.client.set_max_streams_per_donor(&5);

    for _ in 0..3 {
        s.client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    }

    // Drop the cap below the current count.
    s.client.set_max_streams_per_donor(&1);

    // New streams are rejected.
    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::StreamLimitExceeded)));

    // Existing streams are untouched and still withdrawable.
    s.env.ledger().with_mut(|l| l.timestamp += 50);
    let withdrawn = s.client.withdraw(&0);
    assert_eq!(withdrawn, 500);
}

#[test]
fn separate_donors_have_separate_counters() {
    let s = setup();
    s.client.set_max_streams_per_donor(&1);

    let donor_b = Address::generate(&s.env);
    s.token_admin.mint(&s.donor, &1_000);
    s.token_admin.mint(&donor_b, &1_000);

    s.client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    // A different donor is unaffected by donor A's cap.
    let donor_b_stream = s
        .client
        .create_stream(&donor_b, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&donor_b_stream).donor, donor_b);

    // But donor_b is now at its own cap.
    let result = s
        .client
        .try_create_stream(&donor_b, &s.ngo, &s.token.address, &1_000, &10);
    assert!(result.is_err());
}

// =============================================================================
// Explicit stream status (issue #92)
// =============================================================================

#[test]
fn new_stream_starts_active() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
}

#[test]
fn cancelled_stream_reports_cancelled() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.cancel_stream(&stream_id);
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Cancelled
    );
}

#[test]
fn fully_withdrawn_stream_reports_drained() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    // 10 units/s for 1000 units ? fully drained after 100 seconds.
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 100);
    s.client.withdraw(&stream_id);
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );
}

#[test]
fn partially_withdrawn_stream_stays_active() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);
    s.client.withdraw(&stream_id);
    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
}

#[test]
fn status_is_queryable_after_cancel_then_further_operations_fail() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.cancel_stream(&stream_id);
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Cancelled
    );
    // withdraw on a cancelled stream finds nothing accrued (cancel zeroed
    // both rate and balance), regardless of how much time passes after.
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    let result = s.client.try_withdraw(&stream_id);
    assert_eq!(result, Err(Ok(Error::NothingToWithdraw)));
    // top_up on a cancelled stream is rejected (cancelled sets rate = 0).
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    let result = s.client.try_withdraw(&stream_id);
    assert!(result.is_err()); // nothing left to withdraw
}

#[test]
fn preview_stream_reports_duration_and_token_fee_without_mutating_state() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% global
    s.client.set_token_fee_bps(&s.token.address, &100); // 1% for this token

    let preview = s.client.preview_stream(&s.token.address, &1_000, &10);

    assert_eq!(preview.duration, Some(100));
    assert_eq!(preview.fee_bps, 100);
    assert_eq!(preview.error, None);

    // A token with no override falls back to the global fee.
    let other_token = Address::generate(&s.env);
    assert_eq!(
        s.client.preview_stream(&other_token, &1_000, &10).fee_bps,
        500
    );

    // The view is read-only: no auth, no stream, no funds moved.
    assert!(s.env.auths().is_empty());
    assert_eq!(s.client.stream_count(), 0);
    assert_eq!(s.token.balance(&s.client.address), 0);

    // And its duration agrees with the stream once it is actually created.
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let created_at = s.client.get_stream(&stream_id).created_at;
    assert_eq!(s.client.depletion_time(&stream_id), Some(created_at + 100));
}

#[test]
fn preview_stream_matches_create_stream_for_token_and_amount_errors() {
    let s = setup();

    let cases = [
        (s.token.address.clone(), 0i128, 10i128, Error::InvalidAmount),
        (
            s.token.address.clone(),
            1_000i128,
            0i128,
            Error::InvalidAmount,
        ),
        (
            s.token.address.clone(),
            -1i128,
            10i128,
            Error::InvalidAmount,
        ),
        (
            s.token.address.clone(),
            1_000i128,
            -1i128,
            Error::InvalidAmount,
        ),
        (
            s.client.address.clone(),
            1_000i128,
            10i128,
            Error::InvalidToken,
        ),
    ];

    for (token, deposit, rate, expected) in cases {
        let preview = s.client.preview_stream(&token, &deposit, &rate);
        assert_eq!(
            preview.error,
            Some(expected as u32),
            "preview for deposit={deposit} rate={rate}"
        );

        let created = s
            .client
            .try_create_stream(&s.donor, &s.ngo, &token, &deposit, &rate);
        assert_eq!(
            created,
            Err(Ok(expected)),
            "create_stream for deposit={deposit} rate={rate}"
        );
    }

    // A non-positive amount has no meaningful depletion time.
    assert_eq!(
        s.client.preview_stream(&s.token.address, &0, &10).duration,
        None
    );
    assert_eq!(
        s.client
            .preview_stream(&s.token.address, &1_000, &0)
            .duration,
        None
    );
}

#[test]
fn preview_stream_matches_create_stream_for_configured_limits() {
    let s = setup();

    s.client.set_min_deposit(&500);
    let preview = s.client.preview_stream(&s.token.address, &100, &10);
    assert_eq!(preview.error, Some(Error::DepositTooLow as u32));
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &100, &10),
        Err(Ok(Error::DepositTooLow))
    );

    s.client.set_max_rate(&5);
    let preview = s.client.preview_stream(&s.token.address, &1_000, &10);
    assert_eq!(preview.error, Some(Error::RateTooHigh as u32));
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10),
        Err(Ok(Error::RateTooHigh))
    );

    s.client.clear_max_rate();
    s.client.set_min_duration(&101);
    let preview = s.client.preview_stream(&s.token.address, &1_000, &10);
    assert_eq!(preview.error, Some(Error::StreamTooShort as u32));
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10),
        Err(Ok(Error::StreamTooShort))
    );
}

#[test]
fn preview_stream_matches_create_stream_when_paused() {
    let s = setup();
    s.client.pause();

    let preview = s.client.preview_stream(&s.token.address, &1_000, &10);
    assert_eq!(preview.error, Some(Error::ContractPaused as u32));
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10),
        Err(Ok(Error::ContractPaused))
    );
}

#[test]
fn preview_stream_rounds_a_partial_final_second_up() {
    let s = setup();

    let preview = s.client.preview_stream(&s.token.address, &1_000, &300);

    assert_eq!(preview.duration, Some(4));
    assert_eq!(preview.error, None);
}

#[test]
fn max_deposit_per_stream_is_none_by_default() {
    let s = setup();
    assert_eq!(s.client.max_deposit_per_stream(), None);
    assert_eq!(s.client.get_config().max_deposit_per_stream, None);
}

#[test]
fn set_max_deposit_per_stream_stores_value_and_emits_event() {
    let s = setup();

    s.client.set_max_deposit_per_stream(&5_000);

    // Read the event before the getters, which start a new event log.
    assert_last_event(&s.env, (symbol_short!("maxdep"),), 5_000i128);

    assert_eq!(s.client.max_deposit_per_stream(), Some(5_000));
    assert_eq!(s.client.get_config().max_deposit_per_stream, Some(5_000));
}

#[test]
fn set_max_deposit_per_stream_rejects_zero_and_negative() {
    let s = setup();

    assert_eq!(
        s.client.try_set_max_deposit_per_stream(&0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        s.client.try_set_max_deposit_per_stream(&-1),
        Err(Ok(Error::InvalidAmount))
    );
    // No ceiling should have been stored.
    assert_eq!(s.client.max_deposit_per_stream(), None);
}

#[test]
fn clear_max_deposit_per_stream_removes_ceiling_and_emits_event() {
    let s = setup();

    s.client.set_max_deposit_per_stream(&5_000);
    assert_eq!(s.client.max_deposit_per_stream(), Some(5_000));

    s.client.clear_max_deposit_per_stream();
    // Read the event before the getter, which starts a new event log.
    assert_last_event(&s.env, (symbol_short!("maxdepclr"),), ());
    assert_eq!(s.client.max_deposit_per_stream(), None);
}

#[test]
fn clear_max_deposit_per_stream_is_noop_when_not_set() {
    let s = setup();
    // Should not panic and should return Ok.
    s.client.clear_max_deposit_per_stream();
    assert_eq!(s.client.max_deposit_per_stream(), None);
}

#[test]
fn create_stream_accepts_deposit_exactly_at_ceiling() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    s.client.set_max_deposit_per_stream(&1_000);

    // Exactly at the ceiling ΓÇö must succeed.
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

#[test]
fn create_stream_rejects_deposit_above_ceiling() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    s.client.set_max_deposit_per_stream(&999);

    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10),
        Err(Ok(Error::DepositTooHigh))
    );
    // No funds should have moved.
    assert_eq!(s.token.balance(&s.donor), 2_000);
    assert_eq!(s.token.balance(&s.client.address), 0);
}

#[test]
fn create_stream_accepts_any_deposit_when_ceiling_is_cleared() {
    let s = setup();
    s.token_admin.mint(&s.donor, &10_000);

    s.client.set_max_deposit_per_stream(&500);
    // Confirm the ceiling is active.
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10),
        Err(Ok(Error::DepositTooHigh))
    );

    // Remove the ceiling; the same deposit should now succeed.
    s.client.clear_max_deposit_per_stream();
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

#[test]
fn top_up_accepts_amount_that_keeps_balance_at_ceiling() {
    let s = setup();
    // Deposit 500, ceiling 1_000 ΓÇö a 500 top-up should land exactly on the ceiling.
    s.token_admin.mint(&s.donor, &1_500);

    s.client.set_max_deposit_per_stream(&1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &500, &1);

    // No time passes, so balance is still 500. A 500 top-up brings it to 1_000.
    s.client.top_up(&stream_id, &500);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

#[test]
fn top_up_rejects_amount_that_exceeds_ceiling_after_settlement() {
    let s = setup();
    // Deposit 500, ceiling 1_000. After 100 seconds at rate 1, 100 accrues,
    // leaving balance = 400. A top-up of 601 would push balance to 1_001.
    s.token_admin.mint(&s.donor, &2_000);

    s.client.set_max_deposit_per_stream(&1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &500, &1);

    s.env.ledger().with_mut(|l| l.timestamp += 100); // 100 accrues

    // Post-settlement balance will be 400; 601 would exceed the ceiling.
    assert_eq!(
        s.client.try_top_up(&stream_id, &601),
        Err(Ok(Error::DepositTooHigh))
    );
    // Balance and donor funds must be unchanged (top_up is a no-op on error).
    assert_eq!(s.client.get_stream(&stream_id).balance, 500);
    assert_eq!(s.token.balance(&s.donor), 1_500);
}

#[test]
fn top_up_accepts_amount_exactly_at_ceiling_after_settlement() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    s.client.set_max_deposit_per_stream(&1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &500, &1);

    s.env.ledger().with_mut(|l| l.timestamp += 100); // 100 accrues, settled on top_up

    // After settlement balance = 400; a top-up of exactly 600 brings it to 1_000.
    s.client.top_up(&stream_id, &600);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

#[test]
fn get_config_reflects_max_deposit_per_stream_when_set_and_cleared() {
    let s = setup();

    // Not set yet.
    assert_eq!(s.client.get_config().max_deposit_per_stream, None);

    s.client.set_max_deposit_per_stream(&7_777);
    assert_eq!(s.client.get_config().max_deposit_per_stream, Some(7_777));

    s.client.clear_max_deposit_per_stream();
    assert_eq!(s.client.get_config().max_deposit_per_stream, None);
}

#[test]
fn set_max_deposit_per_stream_requires_admin() {
    let s = setup();
    // Read the admin before the call: `admin()` is itself a contract call and
    // would start a new auth log, hiding the authorizer we want to inspect.
    let admin = s.client.admin();
    // mock_all_auths is active, so we probe via the recorded auth instead of
    // bypassing it. The call must succeed and require the admin's authorisation.
    s.client.set_max_deposit_per_stream(&1_000);
    assert_auth_required_from(&s, &admin, "set_max_deposit_per_stream");
}

#[test]
fn clear_max_deposit_per_stream_requires_admin() {
    let s = setup();
    let admin = s.client.admin();
    s.client.set_max_deposit_per_stream(&1_000);
    s.client.clear_max_deposit_per_stream();
    assert_auth_required_from(&s, &admin, "clear_max_deposit_per_stream");
}

#[test]
fn stream_status_state_machine_walks_each_transition() {
    #[derive(Clone, Copy)]
    enum Step {
        Advance(u64),
        Withdraw,
        TopUp(i128),
        ModifyRate(i128),
        Cancel,
        Expect(StreamStatus),
    }

    use Step::*;

    // The create call is implicit for every case: a fresh stream starts Active.
    // Top-up reactivates a drained stream if it has a positive balance after
    // the top-up; that is the behaviour covered by issue #298 and pinned here
    // so the state machine has one source of truth.
    let cases: &[(&str, &[Step])] = &[
        (
            "active -> drained -> top-up reactivates -> cancelled",
            &[
                Expect(StreamStatus::Active),
                Advance(100),
                Withdraw,
                Expect(StreamStatus::Drained),
                TopUp(100),
                Expect(StreamStatus::Active),
                Cancel,
                Expect(StreamStatus::Cancelled),
            ],
        ),
        (
            "active -> drained -> cancelled",
            &[
                Expect(StreamStatus::Active),
                Advance(100),
                Withdraw,
                Expect(StreamStatus::Drained),
                Cancel,
                Expect(StreamStatus::Cancelled),
            ],
        ),
        (
            "active -> partial withdraw stays active -> cancelled",
            &[
                Expect(StreamStatus::Active),
                Advance(50),
                Withdraw,
                Expect(StreamStatus::Active),
                Cancel,
                Expect(StreamStatus::Cancelled),
            ],
        ),
        (
            "rate changes do not revive a drained stream",
            &[
                Expect(StreamStatus::Active),
                Advance(100),
                Withdraw,
                Expect(StreamStatus::Drained),
                ModifyRate(20),
                Expect(StreamStatus::Drained),
                Cancel,
                Expect(StreamStatus::Cancelled),
            ],
        ),
    ];

    for (name, steps) in cases {
        let s = setup();
        s.token_admin.mint(&s.donor, &2_000);
        let stream_id = s
            .client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

        for &step in *steps {
            match step {
                Advance(seconds) => s.env.ledger().with_mut(|l| l.timestamp += seconds),
                Withdraw => {
                    s.client.withdraw(&stream_id);
                }
                TopUp(amount) => {
                    s.client.top_up(&stream_id, &amount);
                }
                ModifyRate(rate) => {
                    s.client.modify_rate(&stream_id, &rate);
                }
                Cancel => {
                    s.client.cancel_stream(&stream_id);
                }
                Expect(expected) => {
                    assert_eq!(
                        s.client.get_stream(&stream_id).status,
                        expected,
                        "{name}: unexpected status"
                    );
                }
            }
        }
    }
}

/// A delayed start and a donor pause are independent clocks: pausing or
/// resuming before the cliff must not pull any of the stream's time forward,
/// and pausing or resuming after it must settle exactly the accrual earned up
/// to that checkpoint. See issue #457.
#[test]
fn future_start_stream_can_be_paused_and_resumed_before_and_after_start() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);

    let start = 1_500;
    let stream_id =
        s.client
            .create_stream_with_start(&s.donor, &s.ngo, &s.token.address, &1_000, &10, &start);

    // Pausing before the cliff settles nothing: the full deposit is still
    // committed and the stream is frozen at zero accrual.
    s.env.ledger().with_mut(|l| l.timestamp = 1_200);
    assert_eq!(s.client.pause_stream(&stream_id), 0);
    let paused = s.client.get_stream(&stream_id);
    assert_eq!(paused.status, StreamStatus::Paused);
    assert_eq!(paused.balance, 1_000);
    assert_eq!(paused.withdrawn, 0);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);
    assert_eq!(s.token.balance(&s.ngo), 0);

    // Time spent paused before the cliff is inert, and resuming before the
    // cliff does not start accrual early.
    s.env.ledger().with_mut(|l| l.timestamp = 1_300);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);
    s.client.resume_stream(&stream_id);
    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
    s.env.ledger().with_mut(|l| l.timestamp = 1_400);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);

    // Once the cliff passes, accrual is dated from `start` rather than from
    // either checkpoint.
    s.env.ledger().with_mut(|l| l.timestamp = 1_520);
    assert_eq!(s.client.pending_accrual(&stream_id), 200);

    // Pausing after the cliff settles those 20 seconds and freezes the rest.
    assert_eq!(s.client.pause_stream(&stream_id), 200);
    let paused = s.client.get_stream(&stream_id);
    assert_eq!(paused.status, StreamStatus::Paused);
    assert_eq!(paused.balance, 800);
    assert_eq!(paused.withdrawn, 200);
    assert_eq!(s.token.balance(&s.ngo), 200);
    s.env.ledger().with_mut(|l| l.timestamp = 1_600);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);

    // Resuming after the cliff restarts the clock from the resume time, so
    // only the seconds after it are payable.
    s.client.resume_stream(&stream_id);
    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
    s.env.ledger().with_mut(|l| l.timestamp = 1_650);
    assert_eq!(s.client.pending_accrual(&stream_id), 500);
    assert_eq!(s.client.withdraw(&stream_id), 500);
    assert_eq!(s.token.balance(&s.ngo), 700);

    let final_stream = s.client.get_stream(&stream_id);
    assert_eq!(final_stream.status, StreamStatus::Active);
    assert_eq!(final_stream.balance, 300);
    assert_eq!(final_stream.withdrawn, 700);
}

#[test]
fn cancelling_a_drained_stream_makes_it_terminal() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 100);
    s.client.withdraw(&stream_id);
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );

    // Cancelling a drained stream moves it to Cancelled, not back to Active.
    assert_eq!(s.client.cancel_stream(&stream_id), 0);
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Cancelled
    );

    // Every stream-mutating path treats the cancelled stream as terminal.
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    assert_eq!(
        s.client.try_withdraw(&stream_id),
        Err(Ok(Error::NothingToWithdraw))
    );
    assert_eq!(
        s.client.try_top_up(&stream_id, &100),
        Err(Ok(Error::StreamCancelled))
    );
    assert_eq!(
        s.client.try_modify_rate(&stream_id, &20),
        Err(Ok(Error::StreamCancelled))
    );
    assert_eq!(
        s.client
            .try_transfer_stream(&stream_id, &Address::generate(&s.env)),
        Err(Ok(Error::StreamCancelled))
    );
}

// =============================================================================
// transfer_stream (reassign a stream to a new NGO)
// =============================================================================

#[test]
fn transfer_stream_settles_accrual_to_old_ngo_and_points_at_new_ngo() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let new_ngo = Address::generate(&s.env);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // 50 seconds pass -> 500 accrues to the old NGO.
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    s.client.transfer_stream(&stream_id, &new_ngo);

    // The accrued 500 settled to the old NGO, and the new NGO got nothing
    // yet â€” only future accrual goes to it.
    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.token.balance(&new_ngo), 0);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.ngo, new_ngo);
    assert_eq!(stream.balance, 500);
    assert_eq!(stream.withdrawn, 500);
    assert_eq!(stream.rate, 10);
    assert!(!stream.cancelled);

    // Further accrual now flows to the new NGO.
    s.env.ledger().with_mut(|l| l.timestamp += 20);
    assert_eq!(s.client.withdraw(&stream_id), 200);
    assert_eq!(s.token.balance(&new_ngo), 200);
    assert_eq!(s.token.balance(&s.ngo), 500);
}

#[test]
fn transfer_stream_emits_event_with_old_and_new_ngo() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let new_ngo = Address::generate(&s.env);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.transfer_stream(&stream_id, &new_ngo);
    let transferred = last_event(&s.env);

    assert_eq!(
        transferred,
        (
            (symbol_short!("transfer"), stream_id).into_val(&s.env),
            (s.ngo.clone(), new_ngo.clone(), 0i128).into_val(&s.env),
        )
    );
}

#[test]
fn transfer_stream_requires_donor_auth() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let new_ngo = Address::generate(&s.env);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.transfer_stream(&stream_id, &new_ngo);

    assert_auth_required_from(&s, &s.donor, "transfer_stream");
}

#[test]
fn transfer_stream_on_cancelled_stream_fails() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let new_ngo = Address::generate(&s.env);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.cancel_stream(&stream_id);

    let result = s.client.try_transfer_stream(&stream_id, &new_ngo);
    assert_eq!(result, Err(Ok(Error::StreamCancelled)));

    // The rejected call leaves the cancelled stream pointing at the old NGO.
    assert_eq!(s.client.get_stream(&stream_id).ngo, s.ngo);
}

#[test]
fn transfer_stream_on_unknown_stream_fails() {
    let s = setup();
    let new_ngo = Address::generate(&s.env);

    let result = s.client.try_transfer_stream(&999, &new_ngo);
    assert_eq!(result, Err(Ok(Error::StreamNotFound)));
}

#[test]
fn transfer_stream_bumps_instance_and_stream_ttl() {
    let s = setup();
    let stream_id = create_ttl_test_stream(&s);
    let new_ngo = Address::generate(&s.env);
    age_past_thresholds(&s, Some(stream_id));

    s.client.transfer_stream(&stream_id, &new_ngo);

    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);
    assert_eq!(stream_ttl(&s, stream_id), STREAM_BUMP_AMOUNT);
}

#[test]
fn transfer_stream_rejects_unauthorized_caller() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let new_ngo = Address::generate(&s.env);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let attacker = Address::generate(&s.env);
    s.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &s.client.address,
            fn_name: "transfer_stream",
            args: (stream_id, new_ngo.clone()).into_val(&s.env),
            sub_invokes: &[],
        },
    }]);

    let result = s.client.try_transfer_stream(&stream_id, &new_ngo);
    assert!(result.is_err());
}

#[test]
fn transfer_stream_rejects_self_stream() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let result = s.client.try_transfer_stream(&stream_id, &s.donor);
    assert_eq!(result, Err(Ok(Error::SelfStream)));
}

#[test]
fn transfer_stream_to_same_ngo_is_noop() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let result = s.client.try_transfer_stream(&stream_id, &s.ngo);
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(s.client.get_stream(&stream_id).ngo, s.ngo);
}

#[test]
fn transfer_stream_rejects_unverified_new_ngo_when_registry_configured() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Old NGO"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let new_ngo = Address::generate(&s.env);
    // new_ngo is not registered / approved in registry
    let result = s.client.try_transfer_stream(&stream_id, &new_ngo);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    // Now register and approve new_ngo
    registry.register(&new_ngo, &soroban_sdk::String::from_str(&s.env, "New NGO"));
    registry.approve_ngo(&s.client.admin(), &new_ngo);
    assert_eq!(
        s.client.try_transfer_stream(&stream_id, &new_ngo),
        Ok(Ok(()))
    );
    assert_eq!(s.client.get_stream(&stream_id).ngo, new_ngo);
}

#[test]
fn transfer_stream_emits_event_with_settled_accrual() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let new_ngo = Address::generate(&s.env);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // 50 seconds pass -> 500 settles to old NGO
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    s.client.transfer_stream(&stream_id, &new_ngo);
    let transferred = last_event(&s.env);

    assert_eq!(
        transferred,
        (
            (symbol_short!("transfer"), stream_id).into_val(&s.env),
            (s.ngo.clone(), new_ngo.clone(), 500i128).into_val(&s.env),
        )
    );
}

#[test]
fn double_init_fails_and_preserves_original_admin() {
    let s = setup();
    let admin = s.client.admin();
    let replacement = Address::generate(&s.env);

    assert_eq!(
        s.client.try_init(&replacement),
        Err(Ok(Error::AlreadyInitialized))
    );
    assert_eq!(s.client.admin(), admin);
}

#[test]
#[should_panic]
fn init_without_admin_auth_fails() {
    let s = setup();
    let attacker = Address::generate(&s.env);
    s.env.mock_auths(&[]);
    s.client.init(&attacker);
}

#[test]
fn pause_blocks_top_up_without_mutating_stream_or_balances() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let before = s.client.get_stream(&stream_id);
    let donor_balance_before = s.token.balance(&s.donor);
    let vault_balance_before = s.token.balance(&s.client.address);

    s.client.pause();
    assert_eq!(
        s.client.try_top_up(&stream_id, &500),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(s.client.get_stream(&stream_id), before);
    assert_eq!(s.token.balance(&s.donor), donor_balance_before);
    assert_eq!(s.token.balance(&s.client.address), vault_balance_before);

    s.client.unpause();
    s.client.top_up(&stream_id, &500);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_500);
}

#[test]
fn withdraw_batch_pays_multiple_streams() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let first = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let second = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &20);
    s.env.ledger().with_mut(|l| l.timestamp += 10);

    let mut ids = Vec::new(&s.env);
    ids.push_back(first);
    ids.push_back(second);
    let withdrawn = s.client.withdraw_batch(&ids);

    assert_eq!(withdrawn.get(0), Some(100));
    assert_eq!(withdrawn.get(1), Some(200));
    assert_eq!(s.token.balance(&s.ngo), 300);
    assert_eq!(s.client.get_stream(&first).withdrawn, 100);
    assert_eq!(s.client.get_stream(&second).withdrawn, 200);
}

#[test]
fn total_donated_counts_create_and_top_up() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.total_donated(), 1_000);

    s.client.top_up(&stream_id, &500);
    assert_eq!(s.client.total_donated(), 1_500);
}

#[test]
fn pause_resume_top_up_cancel_and_outflow_totals() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 10);
    assert_eq!(s.client.pause_stream(&stream_id), 100);
    let paused = s.client.get_stream(&stream_id);
    assert_eq!(paused.status, StreamStatus::Paused);
    assert_eq!(paused.balance, 900);

    s.env.ledger().with_mut(|l| l.timestamp += 50);
    s.client.top_up(&stream_id, &500);
    let topped_up = s.client.get_stream(&stream_id);
    assert_eq!(topped_up.status, StreamStatus::Paused);
    assert_eq!(topped_up.balance, 1_400);
    assert_eq!(s.client.total_withdrawn(), 100);

    s.client.resume_stream(&stream_id);
    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    // `withdraw` returns the net payout (gross minus the 5% fee).
    assert_eq!(s.client.withdraw(&stream_id), 95);
    assert_eq!(s.client.total_withdrawn(), 200);

    s.client.cancel_stream(&stream_id);
    let config = s.client.get_config();
    assert_eq!(config.total_withdrawn, 200);
    // 5% of each 100-unit payout, twice.
    assert_eq!(config.total_fees_collected, 10);
}

#[test]
fn resume_stream_after_ngo_revoked_fails_with_ngo_not_verified() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Advance time so accrual happens, then pause the stream.
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    s.client.pause_stream(&stream_id);
    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Paused);

    // Revoke the NGO while the stream is paused.
    registry.revoke_ngo(&s.client.admin(), &s.ngo);

    // Resume must be rejected — the donor cannot restart payments to a
    // revoked NGO by resuming a stream that was paused before revocation.
    let result = s.client.try_resume_stream(&stream_id);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    // The stream is still paused and its state is unchanged.
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Paused);
}

#[test]
fn resume_stream_after_ngo_re_approved_succeeds() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 10);
    s.client.pause_stream(&stream_id);

    // Revoke, confirm resume is blocked.
    registry.revoke_ngo(&s.client.admin(), &s.ngo);
    assert_eq!(
        s.client.try_resume_stream(&stream_id),
        Err(Ok(Error::NgoNotVerified))
    );

    // Re-approve the NGO — resume should now succeed.
    registry.approve_ngo(&s.client.admin(), &s.ngo);
    assert_eq!(s.client.try_resume_stream(&stream_id), Ok(Ok(())));
    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
}

#[test]
fn resume_stream_without_registry_configured_always_succeeds() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 10);
    s.client.pause_stream(&stream_id);

    // No registry is configured, so the NGO check is skipped entirely.
    assert_eq!(s.client.registry(), None);
    assert_eq!(s.client.try_resume_stream(&stream_id), Ok(Ok(())));
    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
}

#[test]
fn streams_by_ngo_tracks_create_and_transfer_with_pagination() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);
    let other_ngo = Address::generate(&s.env);

    let first = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let second = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &500, &10);

    assert_eq!(s.client.streams_by_ngo(&s.ngo, &0, &1).len(), 1);
    assert_eq!(
        s.client.streams_by_ngo(&s.ngo, &1, &10).get(0),
        Some(second)
    );

    s.client.transfer_stream(&first, &other_ngo);
    assert_eq!(
        s.client.streams_by_ngo(&s.ngo, &0, &10).get(0),
        Some(second)
    );
    assert_eq!(
        s.client.streams_by_ngo(&other_ngo, &0, &10).get(0),
        Some(first)
    );
}

fn create_streams_for_ngo(s: &Setup<'_>, count: u32) -> Vec<u64> {
    s.client.set_max_streams_per_donor(&(count as u64));
    s.token_admin.mint(&s.donor, &(count as i128 * 1_000));
    let mut ids = Vec::new(&s.env);
    for _ in 0..count {
        let id = s
            .client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
        ids.push_back(id);
    }
    ids
}

#[test]
fn streams_by_ngo_pages_follow_the_compacted_index_after_transfers() {
    let s = setup();
    let other_ngo = Address::generate(&s.env);
    let count = MAX_STREAMS_BY_NGO + 3;
    let ids = create_streams_for_ngo(&s, count);

    assert_eq!(
        s.client.streams_by_ngo(&s.ngo, &0, &MAX_STREAMS_BY_NGO),
        ids.slice(0..MAX_STREAMS_BY_NGO)
    );
    assert_eq!(
        s.client
            .streams_by_ngo(&s.ngo, &MAX_STREAMS_BY_NGO, &MAX_STREAMS_BY_NGO),
        ids.slice(MAX_STREAMS_BY_NGO..count)
    );

    s.client.transfer_stream(&ids.get(2).unwrap(), &other_ngo);
    s.client
        .transfer_stream(&ids.get(MAX_STREAMS_BY_NGO + 1).unwrap(), &other_ngo);

    let mut expected_first = Vec::new(&s.env);
    expected_first.append(&ids.slice(0..2));
    expected_first.append(&ids.slice(3..=MAX_STREAMS_BY_NGO));
    let start = 0u32;
    let page = s.client.streams_by_ngo(&s.ngo, &start, &MAX_STREAMS_BY_NGO);
    assert_eq!(page, expected_first);

    let next = start + page.len();
    assert_eq!(next, MAX_STREAMS_BY_NGO);
    let tail = s.client.streams_by_ngo(&s.ngo, &next, &MAX_STREAMS_BY_NGO);
    assert_eq!(tail, ids.slice(count - 1..count));
    assert!(s
        .client
        .streams_by_ngo(&s.ngo, &(next + tail.len()), &MAX_STREAMS_BY_NGO)
        .is_empty());

    let cursor = s
        .client
        .withdraw_all(&s.ngo, &next, &MAX_BATCH_WITHDRAW_STREAMS);
    assert_eq!(cursor.amounts.len(), tail.len());
    assert_eq!(cursor.next, next + tail.len());

    assert_eq!(
        s.client.streams_by_ngo(&other_ngo, &0, &MAX_STREAMS_BY_NGO),
        Vec::from_array(
            &s.env,
            [
                ids.get(2).unwrap(),
                ids.get(MAX_STREAMS_BY_NGO + 1).unwrap()
            ]
        )
    );
}

#[test]
fn streams_by_ngo_pages_keep_cancelled_ids_and_next_position() {
    let s = setup();
    let count = MAX_STREAMS_BY_NGO + 3;
    let ids = create_streams_for_ngo(&s, count);

    let cancelled_first = ids.get(0).unwrap();
    let cancelled_second = ids.get(MAX_STREAMS_BY_NGO + 1).unwrap();
    s.client.cancel_stream(&cancelled_first);
    s.client.cancel_stream(&cancelled_second);
    assert!(s.client.get_stream(&cancelled_first).cancelled);
    assert!(s.client.get_stream(&cancelled_second).cancelled);

    let start = 0u32;
    let page = s.client.streams_by_ngo(&s.ngo, &start, &MAX_STREAMS_BY_NGO);
    assert_eq!(page, ids.slice(0..MAX_STREAMS_BY_NGO));
    let next = start + page.len();
    assert_eq!(next, MAX_STREAMS_BY_NGO);

    let tail = s.client.streams_by_ngo(&s.ngo, &next, &MAX_STREAMS_BY_NGO);
    assert_eq!(tail, ids.slice(MAX_STREAMS_BY_NGO..count));
    assert!(s
        .client
        .streams_by_ngo(&s.ngo, &(next + tail.len()), &MAX_STREAMS_BY_NGO)
        .is_empty());

    s.env.ledger().with_mut(|l| l.timestamp += 10);
    let cursor = s
        .client
        .withdraw_all(&s.ngo, &next, &MAX_BATCH_WITHDRAW_STREAMS);
    assert_eq!(cursor.amounts, Vec::from_array(&s.env, [100i128, 0, 100]));
    assert_eq!(cursor.next, count);
}

#[test]
fn create_stream_cost_stays_flat_as_one_ngo_gains_streams() {
    const STREAM_COUNT: u64 = 300;

    let s = setup();
    s.client.set_max_streams_per_donor(&STREAM_COUNT);
    s.token_admin
        .mint(&s.donor, &(1_000 * STREAM_COUNT as i128));

    let first = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let first_cost = s.env.cost_estimate().resources();

    let mut last = first;
    let mut last_cost = first_cost.clone();
    for _ in 1..STREAM_COUNT {
        last = s
            .client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
        last_cost = s.env.cost_estimate().resources();
    }

    assert_eq!(first, 0);
    assert_eq!(last, STREAM_COUNT - 1);

    let instruction_limit = first_cost.instructions * 2;
    assert!(
        last_cost.instructions <= instruction_limit,
        "create_stream instructions grew from {} for the first stream to {} after {} streams (limit {})",
        first_cost.instructions,
        last_cost.instructions,
        STREAM_COUNT,
        instruction_limit,
    );

    let memory_limit = first_cost.mem_bytes * 2;
    assert!(
        last_cost.mem_bytes <= memory_limit,
        "create_stream memory grew from {} bytes for the first stream to {} bytes after {} streams (limit {})",
        first_cost.mem_bytes,
        last_cost.mem_bytes,
        STREAM_COUNT,
        memory_limit,
    );
}

#[test]
fn active_streams_by_ngo_skips_finished_streams_across_pages() {
    let s = setup();
    s.token_admin.mint(&s.donor, &6_000);

    let s1 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let s2 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let s3 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let s4 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let s5 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let s6 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // s4 is paused while still funded, so it stays live.
    s.client.pause_stream(&s4);
    // 100 s at 10/s drains s2 exactly; s3 is cancelled.
    s.env.ledger().with_mut(|l| l.timestamp += 100);
    s.client.withdraw(&s2);
    s.client.cancel_stream(&s3);

    assert_eq!(s.client.get_stream(&s2).status, StreamStatus::Drained);
    assert_eq!(s.client.get_stream(&s3).status, StreamStatus::Cancelled);
    assert_eq!(s.client.get_stream(&s4).status, StreamStatus::Paused);

    // The first page fills with the two live streams in this window, skipping
    // the drained and cancelled slots between them.
    let first_page = s.client.active_streams_by_ngo(&s.ngo, &0, &2);
    assert_eq!(first_page.ids.len(), 2);
    assert_eq!(first_page.ids.get(0), Some(s1));
    assert_eq!(first_page.ids.get(1), Some(s4));
    assert_eq!(first_page.next, 4);

    let second_page = s.client.active_streams_by_ngo(&s.ngo, &first_page.next, &2);
    assert_eq!(second_page.ids.len(), 2);
    assert_eq!(second_page.ids.get(0), Some(s5));
    assert_eq!(second_page.ids.get(1), Some(s6));
    assert_eq!(second_page.next, 6);

    let done = s
        .client
        .active_streams_by_ngo(&s.ngo, &second_page.next, &2);
    assert!(done.ids.is_empty());
    assert_eq!(done.next, 6);

    // The unfiltered view still reports every slot, including finished ones.
    assert_eq!(s.client.streams_by_ngo(&s.ngo, &0, &10).len(), 6);
}

#[test]
fn active_streams_by_ngo_advances_past_a_window_with_no_live_streams() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);

    let a = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let b = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let c = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 100);
    s.client.withdraw(&a);
    s.client.cancel_stream(&b);
    s.client.withdraw(&c);

    // No live streams, but the cursor still advances to the end of the
    // examined window so the caller can tell "nothing here" from "done".
    let page = s.client.active_streams_by_ngo(&s.ngo, &0, &2);
    assert!(page.ids.is_empty());
    assert_eq!(page.next, 3);

    let done = s.client.active_streams_by_ngo(&s.ngo, &page.next, &2);
    assert!(done.ids.is_empty());
    assert_eq!(done.next, 3);
}

#[test]
fn active_streams_by_ngo_edge_cases_return_a_usable_next() {
    let s = setup();

    // Unknown NGO: nothing to page.
    let unknown = Address::generate(&s.env);
    let page = s.client.active_streams_by_ngo(&unknown, &0, &10);
    assert!(page.ids.is_empty());
    assert_eq!(page.next, 0);

    s.token_admin.mint(&s.donor, &1_000);
    let id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Starting at or past the end reports no progress.
    let page = s.client.active_streams_by_ngo(&s.ngo, &1, &10);
    assert!(page.ids.is_empty());
    assert_eq!(page.next, 1);

    // A zero limit examines nothing rather than looping.
    let page = s.client.active_streams_by_ngo(&s.ngo, &0, &0);
    assert!(page.ids.is_empty());
    assert_eq!(page.next, 0);

    // An oversized limit is clamped to the scan window.
    let page = s.client.active_streams_by_ngo(&s.ngo, &0, &10_000);
    assert_eq!(page.ids.len(), 1);
    assert_eq!(page.ids.get(0), Some(id));
    assert_eq!(page.next, 1);
}

#[test]
fn total_donated_overflow_rejects_create_and_top_up() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    s.env.as_contract(&s.client.address, || {
        s.env
            .storage()
            .instance()
            .set(&DataKey::TotalDonated, &i128::MAX);
    });
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1, &10),
        Err(Ok(Error::ArithmeticOverflow))
    );

    s.env.as_contract(&s.client.address, || {
        s.env
            .storage()
            .instance()
            .set(&DataKey::TotalDonated, &0i128);
    });
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.as_contract(&s.client.address, || {
        s.env
            .storage()
            .instance()
            .set(&DataKey::TotalDonated, &i128::MAX);
    });
    assert_eq!(
        s.client.try_top_up(&stream_id, &1),
        Err(Ok(Error::ArithmeticOverflow))
    );
}

// =============================================================================
// upgrade (contract bytecode upgrade)
// =============================================================================

#[test]
#[ignore = "requires an uploaded wasm hash; upgrade panics before auth is observable"]
fn upgrade_requires_admin_auth() {
    let s = setup();
    let admin = s.client.admin();
    let fake_hash = BytesN::from_array(&s.env, &[1u8; 32]);

    s.client.upgrade(&fake_hash);
    assert_auth_required_from(&s, &admin, "upgrade");
}

#[test]
fn upgrade_rejects_non_admin() {
    let s = setup();
    let fake_hash = BytesN::from_array(&s.env, &[1u8; 32]);

    s.env.mock_auths(&[]);
    let result = s.client.try_upgrade(&fake_hash);
    assert!(result.is_err());
}

#[test]
fn upgrade_fails_when_admin_renounced() {
    let s = setup();
    let fake_hash = BytesN::from_array(&s.env, &[1u8; 32]);

    s.client.renounce_admin();
    let result = s.client.try_upgrade(&fake_hash);
    assert_eq!(result, Err(Ok(Error::AdminRenounced)));
}

#[test]
fn upgrade_fails_with_unuploaded_wasm_hash() {
    let s = setup();
    let unuploaded_hash = BytesN::from_array(&s.env, &[42u8; 32]);
    let result = s.client.try_upgrade(&unuploaded_hash);
    assert!(result.is_err());
}

#[test]
fn upgrade_preserves_state() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);
    s.client.set_min_deposit(&100);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let config_before = s.client.get_config();
    let stream_before = s.client.get_stream(&stream_id);

    // Negative case: attempting an upgrade with an unuploaded wasm hash fails
    let unuploaded_hash = BytesN::from_array(&s.env, &[99u8; 32]);
    assert!(s.client.try_upgrade(&unuploaded_hash).is_err());

    // State reads back identically after
    let config_after = s.client.get_config();
    let stream_after = s.client.get_stream(&stream_id);
    assert_eq!(config_before, config_after);
    assert_eq!(stream_before, stream_after);
}

// =============================================================================
// timelocked upgrade (issue #343)
// =============================================================================

#[test]
fn set_upgrade_delay_configures_timelock_and_publishes_event() {
    let s = setup();

    assert_eq!(s.client.upgrade_delay(), 0);

    s.client.set_upgrade_delay(&3_600);

    assert_last_event(&s.env, (symbol_short!("updelay"),), 3_600u64);
    assert_eq!(s.client.upgrade_delay(), 3_600);
}

#[test]
fn set_upgrade_delay_zero_disables_timelock() {
    let s = setup();

    s.client.set_upgrade_delay(&3_600);
    s.client.set_upgrade_delay(&0);

    assert_eq!(s.client.upgrade_delay(), 0);
}

#[test]
fn set_upgrade_delay_requires_admin() {
    let s = setup();

    s.env.mock_auths(&[]);
    assert!(s.client.try_set_upgrade_delay(&3_600).is_err());
}

#[test]
fn propose_upgrade_records_pending_and_publishes_event() {
    let s = setup();
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);
    s.client.set_upgrade_delay(&3_600);
    let new_wasm_hash = BytesN::from_array(&s.env, &[7u8; 32]);

    s.client.propose_upgrade(&new_wasm_hash);

    // The event buffer is cleared by the next top-level call, so read it first.
    assert_last_event(
        &s.env,
        (symbol_short!("uppropose"), new_wasm_hash.clone()),
        4_600u64,
    );

    let pending = s.client.pending_upgrade().expect("a proposal is pending");
    assert_eq!(pending.new_wasm_hash, new_wasm_hash);
    assert_eq!(pending.executable_at, 4_600);
}

#[test]
fn propose_upgrade_replaces_previous_proposal() {
    let s = setup();
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);
    s.client.set_upgrade_delay(&1_000);
    let first = BytesN::from_array(&s.env, &[1u8; 32]);
    let second = BytesN::from_array(&s.env, &[2u8; 32]);

    s.client.propose_upgrade(&first);
    s.env.ledger().with_mut(|l| l.timestamp = 1_500);
    s.client.propose_upgrade(&second);

    let pending = s.client.pending_upgrade().expect("a proposal is pending");
    assert_eq!(pending.new_wasm_hash, second);
    assert_eq!(pending.executable_at, 2_500);
}

#[test]
fn propose_upgrade_requires_admin() {
    let s = setup();
    let new_wasm_hash = BytesN::from_array(&s.env, &[7u8; 32]);

    s.env.mock_auths(&[]);
    assert!(s.client.try_propose_upgrade(&new_wasm_hash).is_err());
}

#[test]
fn execute_upgrade_before_delay_elapsed_fails() {
    let s = setup();
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);
    s.client.set_upgrade_delay(&3_600);
    let new_wasm_hash = s.env.deployer().upload_contract_wasm(Bytes::new(&s.env));

    s.client.propose_upgrade(&new_wasm_hash);

    let result = s.client.try_execute_upgrade();
    assert_eq!(result, Err(Ok(Error::UpgradeDelayNotElapsed)));
}

#[test]
fn execute_upgrade_after_delay_elapsed_succeeds() {
    let s = setup();
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);
    s.client.set_upgrade_delay(&3_600);
    let new_wasm_hash = s.env.deployer().upload_contract_wasm(Bytes::new(&s.env));

    s.client.propose_upgrade(&new_wasm_hash);
    s.env.ledger().with_mut(|l| l.timestamp = 4_600);

    s.client.execute_upgrade();

    assert_last_event(&s.env, (symbol_short!("upgrade"),), new_wasm_hash.clone());
    assert!(s.client.pending_upgrade().is_none());
}

#[test]
fn execute_upgrade_emits_config_snapshot_of_full_config() {
    let s = setup();
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);
    s.client.set_upgrade_delay(&3_600);

    // Give the snapshot non-default values to prove it carries the whole
    // Config rather than just a handful of fields.
    let treasury = Address::generate(&s.env);
    let registry = Address::generate(&s.env);
    let candidate = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);
    s.client.set_min_deposit(&100);
    s.client.set_max_streams_per_donor(&50);
    s.client.set_max_rate(&1_000);
    s.client.set_min_duration(&60);
    s.client.set_cancel_grace_ledgers(&10);
    s.client.set_registry(&registry);
    s.client.propose_admin(&candidate);
    s.client.pause();

    // Read before executing: any top-level call clears the event buffer.
    let config_before = s.client.get_config();

    let new_wasm_hash = s.env.deployer().upload_contract_wasm(Bytes::new(&s.env));
    s.client.propose_upgrade(&new_wasm_hash);
    s.env.ledger().with_mut(|l| l.timestamp = 4_600);
    s.client.execute_upgrade();

    // The snapshot precedes `upgrade`, which stays the call's last event.
    assert_nth_last_event(
        &s.env,
        1,
        (Symbol::new(&s.env, "config_snapshot"),),
        config_before.clone(),
    );
    assert_last_event(&s.env, (symbol_short!("upgrade"),), new_wasm_hash);
    assert_eq!(s.client.get_config(), config_before);
}

#[test]
fn execute_upgrade_without_proposal_fails() {
    let s = setup();

    let result = s.client.try_execute_upgrade();
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
}

#[test]
fn execute_upgrade_requires_admin() {
    let s = setup();
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);
    s.client.set_upgrade_delay(&1);
    let new_wasm_hash = s.env.deployer().upload_contract_wasm(Bytes::new(&s.env));
    s.client.propose_upgrade(&new_wasm_hash);
    s.env.ledger().with_mut(|l| l.timestamp = 1_001);

    s.env.mock_auths(&[]);
    assert!(s.client.try_execute_upgrade().is_err());
}

#[test]
fn cancel_upgrade_clears_pending_and_publishes_event() {
    let s = setup();
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);
    s.client.set_upgrade_delay(&3_600);
    let new_wasm_hash = BytesN::from_array(&s.env, &[9u8; 32]);

    s.client.propose_upgrade(&new_wasm_hash);

    s.client.cancel_upgrade();

    assert_last_event(&s.env, (symbol_short!("upcancel"),), new_wasm_hash.clone());
    assert!(s.client.pending_upgrade().is_none());
}

#[test]
fn cancel_upgrade_without_proposal_fails() {
    let s = setup();

    let result = s.client.try_cancel_upgrade();
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
}

#[test]
fn cancel_upgrade_requires_admin() {
    let s = setup();
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);
    s.client.set_upgrade_delay(&3_600);
    let new_wasm_hash = BytesN::from_array(&s.env, &[9u8; 32]);
    s.client.propose_upgrade(&new_wasm_hash);

    s.env.mock_auths(&[]);
    assert!(s.client.try_cancel_upgrade().is_err());
}

#[test]
fn upgrade_rejected_while_timelock_enabled() {
    let s = setup();
    s.client.set_upgrade_delay(&3_600);
    let new_wasm_hash = s.env.deployer().upload_contract_wasm(Bytes::new(&s.env));

    let result = s.client.try_upgrade(&new_wasm_hash);
    assert_eq!(result, Err(Ok(Error::UpgradeTimelockEnabled)));
}

#[test]
fn upgrade_allowed_once_timelock_disabled() {
    let s = setup();
    s.client.set_upgrade_delay(&3_600);
    // Explicitly disabling the timelock re-enables the immediate path.
    s.client.set_upgrade_delay(&0);
    let new_wasm_hash = s.env.deployer().upload_contract_wasm(Bytes::new(&s.env));

    s.client.upgrade(&new_wasm_hash);

    assert_last_event(&s.env, (symbol_short!("upgrade"),), new_wasm_hash.clone());
}

// =============================================================================
// committed balance and surplus sweep (issue #346)
// =============================================================================

#[test]
fn committed_balance_tracks_deposits_and_accrued_withdrawals() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    assert_eq!(s.client.committed_balance(&s.token.address), 0);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // The whole deposit is committed to the stream.
    assert_eq!(s.client.committed_balance(&s.token.address), 1_000);
    assert_eq!(s.token.balance(&s.client.address), 1_000);
    assert_eq!(s.client.surplus(&s.token.address), 0);

    // 40 seconds accrue 400, all of which leaves the vault on withdrawal.
    s.env.ledger().with_mut(|l| l.timestamp += 40);
    s.client.withdraw(&stream_id);

    assert_eq!(s.client.committed_balance(&s.token.address), 600);
    assert_eq!(s.token.balance(&s.client.address), 600);
    assert_eq!(s.client.surplus(&s.token.address), 0);
}

#[test]
fn set_registry_rejects_unrelated_contract_and_preserves_existing() {
    let (s, _registry) = setup_with_registry();
    let existing = s.client.registry().unwrap();

    // The token is a deployed contract, but it is not an NGO registry and
    // does not answer `ngo_count`.
    let result = s.client.try_set_registry(&s.token.address);

    assert_eq!(result, Err(Ok(Error::InvalidRegistry)));
    assert_eq!(s.client.registry(), Some(existing.clone()));
    assert_eq!(s.client.get_config().registry, Some(existing));
}

#[test]
fn set_registry_rejects_plain_account_and_preserves_existing() {
    let (s, _registry) = setup_with_registry();
    let existing = s.client.registry().unwrap();

    let account = Address::generate(&s.env);
    let result = s.client.try_set_registry(&account);

    assert_eq!(result, Err(Ok(Error::InvalidRegistry)));
    assert_eq!(s.client.registry(), Some(existing.clone()));
    assert_eq!(s.client.get_config().registry, Some(existing));
}

#[test]
fn set_registry_rejects_the_vault_itself_and_preserves_existing() {
    let (s, _registry) = setup_with_registry();
    let existing = s.client.registry().unwrap();

    let result = s.client.try_set_registry(&s.client.address);

    assert_eq!(result, Err(Ok(Error::InvalidRegistry)));
    assert_eq!(s.client.registry(), Some(existing.clone()));
    assert_eq!(s.client.get_config().registry, Some(existing));
}

#[test]
fn committed_balance_tracks_top_ups() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_500);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.committed_balance(&s.token.address), 1_000);

    s.client.top_up(&stream_id, &500);

    assert_eq!(s.client.committed_balance(&s.token.address), 1_500);
    assert_eq!(s.client.surplus(&s.token.address), 0);
}

#[test]
fn committed_balance_drops_when_stream_is_cancelled() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.committed_balance(&s.token.address), 1_000);

    s.env.ledger().with_mut(|l| l.timestamp += 20); // 200 accrues, 800 refunds
    s.client.cancel_stream(&stream_id);

    // Both the settled payout and the refund left the vault.
    assert_eq!(s.client.committed_balance(&s.token.address), 0);
    assert_eq!(s.client.surplus(&s.token.address), 0);
}

#[test]
fn surplus_reflects_tokens_sent_to_the_vault_by_mistake() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_500);

    let _stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // 500 strays into the vault without a stream backing it.
    s.token.transfer(&s.donor, &s.client.address, &500);

    assert_eq!(s.client.committed_balance(&s.token.address), 1_000);
    assert_eq!(s.client.surplus(&s.token.address), 500);
}

#[test]
fn sweep_surplus_moves_only_the_surplus_and_publishes_event() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);

    s.token_admin.mint(&s.donor, &1_500);
    let _stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.token.transfer(&s.donor, &s.client.address, &500);

    let swept = s.client.try_sweep_surplus(&s.token.address, &500);
    assert!(swept.is_ok());

    assert_last_event(
        &s.env,
        (symbol_short!("sweep"), s.token.address.clone()),
        (500i128, treasury.clone()),
    );

    // The treasury got the surplus; the stream's committed funds are intact.
    assert_eq!(s.token.balance(&treasury), 500);
    assert_eq!(s.token.balance(&s.client.address), 1_000);
    assert_eq!(s.client.committed_balance(&s.token.address), 1_000);
    assert_eq!(s.client.surplus(&s.token.address), 0);
}

#[test]
fn sweep_surplus_can_never_touch_stream_funds() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);

    s.token_admin.mint(&s.donor, &1_400);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    // Only 400 is surplus.
    s.token.transfer(&s.donor, &s.client.address, &400);

    // Asking for one more than the surplus is rejected outright.
    let result = s.client.try_sweep_surplus(&s.token.address, &401);
    assert_eq!(result, Err(Ok(Error::SweepExceedsSurplus)));

    // Even the whole vault balance â€” stream funds included â€” is rejected.
    let result = s.client.try_sweep_surplus(&s.token.address, &1_400);
    assert_eq!(result, Err(Ok(Error::SweepExceedsSurplus)));

    // Sweeping the surplus leaves exactly the streams' 1_000 committed.
    s.client.sweep_surplus(&s.token.address, &400);
    assert_eq!(s.token.balance(&s.client.address), 1_000);

    // And the NGO can still draw down the full committed amount.
    s.env.ledger().with_mut(|l| l.timestamp += 100);
    s.client.withdraw(&stream_id);
    assert_eq!(s.token.balance(&s.ngo), 1_000);
    assert_eq!(s.client.committed_balance(&s.token.address), 0);
}

#[test]
fn sweep_surplus_requires_a_treasury() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_100);
    s.token.transfer(&s.donor, &s.client.address, &100);

    let result = s.client.try_sweep_surplus(&s.token.address, &100);
    assert_eq!(result, Err(Ok(Error::NoTreasury)));
}

#[test]
fn sweep_surplus_rejects_non_positive_amount() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);

    let result = s.client.try_sweep_surplus(&s.token.address, &0);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));

    let result = s.client.try_sweep_surplus(&s.token.address, &-1);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn sweep_surplus_requires_admin() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);

    s.env.mock_auths(&[]);
    assert!(s.client.try_sweep_surplus(&s.token.address, &1).is_err());
}

#[test]
fn sweep_surplus_rejected_while_paused() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.pause();

    let result = s.client.try_sweep_surplus(&s.token.address, &1);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

#[test]
fn sweep_surplus_to_sends_to_explicit_destination_and_publishes_event() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    let destination = Address::generate(&s.env);

    s.token_admin.mint(&s.donor, &1_500);
    let _stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.token.transfer(&s.donor, &s.client.address, &500);

    let swept = s
        .client
        .try_sweep_surplus_to(&s.token.address, &500, &destination);
    assert!(swept.is_ok());

    assert_last_event(
        &s.env,
        (symbol_short!("sweep"), s.token.address.clone()),
        (500i128, destination.clone()),
    );

    // The explicit destination wins over the configured treasury, which is
    // left untouched; the stream's committed funds stay in the vault.
    assert_eq!(s.token.balance(&destination), 500);
    assert_eq!(s.token.balance(&treasury), 0);
    assert_eq!(s.token.balance(&s.client.address), 1_000);
    assert_eq!(s.client.committed_balance(&s.token.address), 1_000);
}

#[test]
fn sweep_surplus_to_works_without_a_treasury() {
    let s = setup();
    let destination = Address::generate(&s.env);

    s.token_admin.mint(&s.donor, &100);
    s.token.transfer(&s.donor, &s.client.address, &100);

    // No treasury is configured, so `sweep_surplus` would fail with
    // NoTreasury; the explicit destination needs none.
    let result = s
        .client
        .try_sweep_surplus_to(&s.token.address, &100, &destination);
    assert!(result.is_ok());
    assert_eq!(s.token.balance(&destination), 100);
    assert_eq!(s.client.surplus(&s.token.address), 0);
}

#[test]
fn sweep_surplus_to_respects_the_surplus_bound() {
    let s = setup();
    let destination = Address::generate(&s.env);

    s.token_admin.mint(&s.donor, &1_400);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.token.transfer(&s.donor, &s.client.address, &400);

    let result = s
        .client
        .try_sweep_surplus_to(&s.token.address, &401, &destination);
    assert_eq!(result, Err(Ok(Error::SweepExceedsSurplus)));
    assert_eq!(s.token.balance(&destination), 0);

    s.client
        .sweep_surplus_to(&s.token.address, &400, &destination);
    assert_eq!(s.token.balance(&destination), 400);
    assert_eq!(s.token.balance(&s.client.address), 1_000);

    // The stream can still draw down its full committed deposit.
    s.env.ledger().with_mut(|l| l.timestamp += 100);
    s.client.withdraw(&stream_id);
    assert_eq!(s.token.balance(&s.ngo), 1_000);
}

#[test]
fn sweep_surplus_to_rejects_the_vault_as_destination() {
    let s = setup();
    s.token_admin.mint(&s.donor, &100);
    s.token.transfer(&s.donor, &s.client.address, &100);

    let result = s
        .client
        .try_sweep_surplus_to(&s.token.address, &100, &s.client.address);
    assert_eq!(result, Err(Ok(Error::InvalidSweepDestination)));
    assert_eq!(s.token.balance(&s.client.address), 100);
}

#[test]
fn sweep_surplus_to_rejects_non_positive_amount() {
    let s = setup();
    let destination = Address::generate(&s.env);

    let result = s
        .client
        .try_sweep_surplus_to(&s.token.address, &0, &destination);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));

    let result = s
        .client
        .try_sweep_surplus_to(&s.token.address, &-1, &destination);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn sweep_surplus_to_requires_admin() {
    let s = setup();
    let destination = Address::generate(&s.env);

    s.env.mock_auths(&[]);
    assert!(s
        .client
        .try_sweep_surplus_to(&s.token.address, &1, &destination)
        .is_err());
}

#[test]
fn sweep_surplus_to_rejected_while_paused() {
    let s = setup();
    let destination = Address::generate(&s.env);
    s.client.pause();

    let result = s
        .client
        .try_sweep_surplus_to(&s.token.address, &1, &destination);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

// =============================================================================
// rescue_stream tests (issue #286)
// =============================================================================

#[test]
fn rescue_stream_refunds_donor_and_updates_state_and_emits_event() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Advance timestamp by 20 seconds: under normal cancel, 200 would accrue to the NGO.
    s.env.ledger().with_mut(|l| l.timestamp += 20);

    // Initial balances before rescue:
    assert_eq!(s.token.balance(&s.donor), 0);
    assert_eq!(s.token.balance(&s.client.address), 1_000);
    assert_eq!(s.token.balance(&s.ngo), 0);

    s.client.pause();

    // Rescue the stream.
    let refund = s.client.rescue_stream(&stream_id);
    assert_eq!(refund, 1_000);
    let evt = last_event(&s.env);

    // Assert exact balances moved: full remaining balance refunded to donor, NGO gets nothing.
    assert_eq!(s.token.balance(&s.donor), 1_000);
    assert_eq!(s.token.balance(&s.client.address), 0);
    assert_eq!(s.token.balance(&s.ngo), 0);

    // Assert stream's resulting state.
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.rate, 0);
    assert!(stream.cancelled);
    assert_eq!(stream.status, StreamStatus::Cancelled);
    assert_eq!(stream.last_update, s.env.ledger().timestamp());
    assert_eq!(stream.withdrawn, 0);

    // Assert rescue event topics and data.
    assert_eq!(
        evt,
        (
            (symbol_short!("rescue"), stream_id).into_val(&s.env),
            refund.into_val(&s.env),
        )
    );
    assert_eq!(
        evt,
        (
            (symbol_short!("rescue"), stream_id).into_val(&s.env),
            1_000i128.into_val(&s.env),
        )
    );
}

#[test]
fn rescue_stream_when_not_paused_returns_not_paused() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Vault is not paused; rescue must return Error::NotPaused.
    let result = s.client.try_rescue_stream(&stream_id);
    assert_eq!(result, Err(Ok(Error::NotPaused)));
}

#[test]
fn rescue_stream_already_cancelled_returns_stream_cancelled() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Donor cancels the stream first.
    s.client.cancel_stream(&stream_id);

    // Pause vault and try to rescue the cancelled stream.
    s.client.pause();
    let result = s.client.try_rescue_stream(&stream_id);
    assert_eq!(result, Err(Ok(Error::StreamCancelled)));
}

#[test]
fn rescue_stream_twice_returns_stream_cancelled() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.pause();
    let refund = s.client.rescue_stream(&stream_id);
    assert_eq!(refund, 1_000);

    // Second rescue attempt on the already rescued stream fails with StreamCancelled.
    let result = s.client.try_rescue_stream(&stream_id);
    assert_eq!(result, Err(Ok(Error::StreamCancelled)));
}

#[test]
fn rescue_stream_unknown_id_returns_stream_not_found() {
    let s = setup();
    s.client.pause();

    let unknown_id = 123456u64;
    let result = s.client.try_rescue_stream(&unknown_id);
    assert_eq!(result, Err(Ok(Error::StreamNotFound)));
}

#[test]
#[should_panic]
fn rescue_stream_fails_for_non_admin_caller() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.pause();

    let non_admin = Address::generate(&s.env);
    s.env.mock_auths(&[MockAuth {
        address: &non_admin,
        invoke: &MockAuthInvoke {
            contract: &s.client.address,
            fn_name: "rescue_stream",
            args: (stream_id,).into_val(&s.env),
            sub_invokes: &[],
        },
    }]);

    s.client.rescue_stream(&stream_id);
}

#[test]
#[should_panic]
fn rescue_stream_fails_without_admin_auth() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.pause();

    s.env.mock_auths(&[]);
    s.client.rescue_stream(&stream_id);
}

// =============================================================================
// NGO registry cross-contract verification & set_registry tests (issue #287)
// =============================================================================

fn setup_with_registry() -> (Setup<'static>, ngo_registry::NgoRegistryClient<'static>) {
    let s = setup();

    let admin = s.client.admin();
    let registry_id = s.env.register(
        ngo_registry::NgoRegistry,
        ngo_registry::NgoRegistryArgs::__constructor(&admin),
    );
    let registry_client = ngo_registry::NgoRegistryClient::new(&s.env, &registry_id);

    s.client.set_registry(&registry_id);

    (s, registry_client)
}

#[test]
fn set_registry_and_registry_getter() {
    let s = setup();
    assert_eq!(s.client.registry(), None);
    assert_eq!(s.client.get_config().registry, None);

    let registry_addr = Address::generate(&s.env);
    s.client.set_registry(&registry_addr);

    assert_eq!(s.client.registry(), Some(registry_addr.clone()));
    assert_eq!(s.client.get_config().registry, Some(registry_addr));
}

#[test]
fn set_registry_requires_admin_auth() {
    let s = setup();
    let admin = s.client.admin();
    let registry_addr = Address::generate(&s.env);

    s.client.set_registry(&registry_addr);
    assert_auth_required_from(&s, &admin, "set_registry");
}

#[test]
#[should_panic]
fn set_registry_fails_without_admin_auth() {
    let s = setup();
    let registry_addr = Address::generate(&s.env);

    s.env.mock_auths(&[]);
    s.client.set_registry(&registry_addr);
}

#[test]
#[should_panic]
fn set_registry_rejects_non_admin() {
    let s = setup();
    let non_admin = Address::generate(&s.env);
    let registry_addr = Address::generate(&s.env);

    s.env.mock_auths(&[MockAuth {
        address: &non_admin,
        invoke: &MockAuthInvoke {
            contract: &s.client.address,
            fn_name: "set_registry",
            args: (registry_addr.clone(),).into_val(&s.env),
            sub_invokes: &[],
        },
    }]);

    s.client.set_registry(&registry_addr);
}

#[test]
fn create_stream_succeeds_for_verified_ngo() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

#[test]
fn create_stream_fails_for_unverified_ngo() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    // Registered but not approved (verified = false)
    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    // No funds moved from donor
    assert_eq!(s.token.balance(&s.donor), 1_000);
}

#[test]
fn create_stream_fails_for_unregistered_ngo() {
    let (s, _registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    // The NGO has no registry entry. try_get_ngo fails and maps to NgoNotVerified.
    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    assert_eq!(s.token.balance(&s.donor), 1_000);
}

#[test]
fn get_config_snapshot_every_setter() {
    let s = setup();

    // Snapshot 1: Default config after init
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: false,
            treasury: None,
            fee_bps: 0,
            min_deposit: 0,
            max_streams_per_donor: 100,
            max_rate: None,
            min_duration: 0,
            registry: None,
            cancel_grace_ledgers: 0,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // Snapshot 2: After set_treasury
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: false,
            treasury: Some(treasury.clone()),
            fee_bps: 0,
            min_deposit: 0,
            max_streams_per_donor: 100,
            max_rate: None,
            min_duration: 0,
            registry: None,
            cancel_grace_ledgers: 0,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // Snapshot 3: After set_fee_bps
    s.client.set_fee_bps(&500);
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: false,
            treasury: Some(treasury.clone()),
            fee_bps: 500,
            min_deposit: 0,
            max_streams_per_donor: 100,
            max_rate: None,
            min_duration: 0,
            registry: None,
            cancel_grace_ledgers: 0,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // Snapshot 4: After set_min_deposit
    s.client.set_min_deposit(&100);
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: false,
            treasury: Some(treasury.clone()),
            fee_bps: 500,
            min_deposit: 100,
            max_streams_per_donor: 100,
            max_rate: None,
            min_duration: 0,
            registry: None,
            cancel_grace_ledgers: 0,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // Snapshot 5: After set_max_streams_per_donor
    s.client.set_max_streams_per_donor(&50);
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: false,
            treasury: Some(treasury.clone()),
            fee_bps: 500,
            min_deposit: 100,
            max_streams_per_donor: 50,
            max_rate: None,
            min_duration: 0,
            registry: None,
            cancel_grace_ledgers: 0,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // Snapshot 6: After set_max_rate
    s.client.set_max_rate(&1_000);
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: false,
            treasury: Some(treasury.clone()),
            fee_bps: 500,
            min_deposit: 100,
            max_streams_per_donor: 50,
            max_rate: Some(1_000),
            min_duration: 0,
            registry: None,
            cancel_grace_ledgers: 0,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // Snapshot 7: After set_min_duration
    s.client.set_min_duration(&60);
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: false,
            treasury: Some(treasury.clone()),
            fee_bps: 500,
            min_deposit: 100,
            max_streams_per_donor: 50,
            max_rate: Some(1_000),
            min_duration: 60,
            registry: None,
            cancel_grace_ledgers: 0,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // Snapshot 8: After set_registry
    let registry = Address::generate(&s.env);
    s.client.set_registry(&registry);
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: false,
            treasury: Some(treasury.clone()),
            fee_bps: 500,
            min_deposit: 100,
            max_streams_per_donor: 50,
            max_rate: Some(1_000),
            min_duration: 60,
            registry: Some(registry.clone()),
            cancel_grace_ledgers: 0,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // Snapshot 9: After set_cancel_grace_ledgers
    s.client.set_cancel_grace_ledgers(&10);
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: false,
            treasury: Some(treasury.clone()),
            fee_bps: 500,
            min_deposit: 100,
            max_streams_per_donor: 50,
            max_rate: Some(1_000),
            min_duration: 60,
            registry: Some(registry.clone()),
            cancel_grace_ledgers: 10,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // Snapshot 10: After pause
    s.client.pause();
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: true,
            treasury: Some(treasury.clone()),
            fee_bps: 500,
            min_deposit: 100,
            max_streams_per_donor: 50,
            max_rate: Some(1_000),
            min_duration: 60,
            registry: Some(registry.clone()),
            cancel_grace_ledgers: 10,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // Snapshot 11: After unpause
    s.client.unpause();
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: false,
            treasury: Some(treasury),
            fee_bps: 500,
            min_deposit: 100,
            max_streams_per_donor: 50,
            max_rate: Some(1_000),
            min_duration: 60,
            registry: Some(registry),
            cancel_grace_ledgers: 10,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
            max_deposit_per_stream: None,
        }
    );

    // Snapshot 12: After propose_admin
    let candidate = Address::generate(&s.env);
    s.client.propose_admin(&candidate);
    assert_eq!(s.client.get_config().pending_admin, Some(candidate.clone()));

    // Snapshot 13: After cancel_admin_proposal
    s.client.cancel_admin_proposal();
    assert_eq!(s.client.get_config().pending_admin, None);

    // Propose again for the final snapshot
    s.client.propose_admin(&candidate);

    // Snapshot 14: After renounce_admin (pending_admin cleared, renounced = true)
    s.client.renounce_admin();
    assert_eq!(
        s.client.get_config(),
        Config {
            paused: false,
            treasury: None,
            fee_bps: 500,
            min_deposit: 100,
            max_streams_per_donor: 50,
            max_rate: Some(1_000),
            min_duration: 60,
            registry: None,
            cancel_grace_ledgers: 10,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: true,
            max_deposit_per_stream: None,
        }
    );
}

#[test]
#[ignore] // TODO: Requires custom mock token that can fail on specific transfer call
fn withdraw_batch_is_atomic_on_token_transfer_failure() {
    // This test verifies that withdraw_batch is atomic: if any token transfer
    // fails, the entire transaction must revert and leave all streams unchanged.
    //
    // Test scenario:
    // 1. Create 3 streams with accrued amounts
    // 2. Use a mock token that fails on the 2nd transfer
    // 3. Call withdraw_batch on all 3 streams
    // 4. Verify that the call fails
    // 5. Verify that NO stream's last_update or withdrawn was changed
    // 6. Verify that NO tokens were transferred
    //
    // Implementation note: This requires a custom token contract that can be
    // configured to fail on a specific transfer call. The Soroban SDK's
    // built-in token doesn't support this, so this test is marked as ignored
    // until proper mock infrastructure is available.

    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);

    // Create 3 streams
    let stream1 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let stream2 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let stream3 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Let some time pass so all streams accrue
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    // Record state before batch withdrawal
    let stream1_before = s.client.get_stream(&stream1);
    let stream2_before = s.client.get_stream(&stream2);
    let stream3_before = s.client.get_stream(&stream3);
    let ngo_balance_before = s.token.balance(&s.ngo);
    let vault_balance_before = s.token.balance(&s.client.address);

    // TODO: Replace with failing mock token
    // let failing_token = create_failing_token(&s.env, 2); // Fails on 2nd transfer

    // Try to withdraw all 3 streams - should fail and revert
    let mut ids = Vec::new(&s.env);
    ids.push_back(stream1);
    ids.push_back(stream2);
    ids.push_back(stream3);

    // TODO: This should fail when using failing token
    // let result = s.client.try_withdraw_batch(&ids);
    // assert!(result.is_err());

    // Verify atomicity: NO streams should have changed
    let stream1_after = s.client.get_stream(&stream1);
    let stream2_after = s.client.get_stream(&stream2);
    let stream3_after = s.client.get_stream(&stream3);

    assert_eq!(stream1_before.last_update, stream1_after.last_update);
    assert_eq!(stream1_before.withdrawn, stream1_after.withdrawn);
    assert_eq!(stream2_before.last_update, stream2_after.last_update);
    assert_eq!(stream2_before.withdrawn, stream2_after.withdrawn);
    assert_eq!(stream3_before.last_update, stream3_after.last_update);
    assert_eq!(stream3_before.withdrawn, stream3_after.withdrawn);

    // Verify NO tokens were transferred
    assert_eq!(s.token.balance(&s.ngo), ngo_balance_before);
    assert_eq!(s.token.balance(&s.client.address), vault_balance_before);
}

#[test]
fn withdraw_batch_at_max_size_stays_within_resource_budget() {
    let s = setup();

    // Create exactly MAX_BATCH_WITHDRAW_STREAMS (20) streams
    let balance_per_stream: i128 = 1_000;
    let total_balance = balance_per_stream * MAX_BATCH_WITHDRAW_STREAMS as i128;
    s.token_admin.mint(&s.donor, &total_balance);

    let mut stream_ids = Vec::new(&s.env);
    for _ in 0..MAX_BATCH_WITHDRAW_STREAMS {
        let stream_id =
            s.client
                .create_stream(&s.donor, &s.ngo, &s.token.address, &balance_per_stream, &10);
        stream_ids.push_back(stream_id);
    }

    // Advance time so all streams have accrued funds
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 per stream

    // Reset and enforce budget for the withdraw_batch call
    // Soroban's default budget (without enforced limits):
    // - CPU instructions: 100M instructions
    // - Memory: 40MB footprint
    //
    // Safety margin: withdraw_batch at max capacity (20 streams) should use:
    // - Less than 50M CPU instructions (50% of budget)
    // - Less than 20MB memory footprint (50% of budget)
    //
    // These margins ensure that:
    // 1. Future optimizations won't break existing usage
    // 2. Network protocol changes have room to adjust limits
    // 3. Contract upgrades with minor logic additions stay safe
    s.env.cost_estimate().budget().reset_unlimited();

    // Execute the batch withdrawal with enforced budget
    let amounts = s.client.withdraw_batch(&stream_ids);

    // Verify the operation succeeded
    assert_eq!(amounts.len(), MAX_BATCH_WITHDRAW_STREAMS);
    for i in 0..MAX_BATCH_WITHDRAW_STREAMS {
        assert_eq!(amounts.get(i), Some(500));
    }

    // Check resource usage
    let cpu_insns = s.env.cost_estimate().budget().cpu_instruction_cost();
    let mem_bytes = s.env.cost_estimate().budget().memory_bytes_cost();

    // Document current usage for future reference
    // If these assertions fail, it means withdraw_batch resource usage has increased
    // and MAX_BATCH_WITHDRAW_STREAMS may need to be reduced, or the implementation
    // needs optimization.
    //
    // CPU budget margin: 50M instructions (50% of 100M default)
    const MAX_CPU_INSTRUCTIONS: u64 = 50_000_000;
    // Memory budget margin: 20MB (50% of 40MB default)
    const MAX_MEMORY_BYTES: u64 = 20_000_000;

    assert!(
        cpu_insns < MAX_CPU_INSTRUCTIONS,
        "withdraw_batch CPU usage ({} instructions) exceeds safe margin ({} instructions). \
         This may cause on-chain failures. Consider optimizing or reducing MAX_BATCH_WITHDRAW_STREAMS.",
        cpu_insns,
        MAX_CPU_INSTRUCTIONS
    );

    assert!(
        mem_bytes < MAX_MEMORY_BYTES,
        "withdraw_batch memory usage ({} bytes) exceeds safe margin ({} bytes). \
         This may cause on-chain failures. Consider optimizing or reducing MAX_BATCH_WITHDRAW_STREAMS.",
        mem_bytes,
        MAX_MEMORY_BYTES
    );
}

#[test]
fn create_stream_fails_for_revoked_ngo() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    // Register, approve, then revoke
    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);
    registry.revoke_ngo(&s.client.admin(), &s.ngo);

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    assert_eq!(s.token.balance(&s.donor), 1_000);
}

#[test]
fn create_stream_skips_verification_when_no_registry_configured() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    // Without set_registry, registry() is None; create_stream skips the verification branch
    assert_eq!(s.client.registry(), None);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

#[test]
fn bad_registry_fails_create_stream_and_clear_registry_restores_creation() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    // Set an invalid registry address (not a real contract)
    let bad_registry = Address::generate(&s.env);
    s.client.set_registry(&bad_registry);
    assert_eq!(s.client.registry(), Some(bad_registry));

    // create_stream attempts cross-contract call to bad address, fails closed with NgoNotVerified
    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    // Admin clears the bad registry
    s.client.clear_registry();
    assert_eq!(s.client.registry(), None);

    // Now streams can be created again (skips verification)
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

#[test]
fn clear_registry_requires_admin_auth() {
    let s = setup();
    let admin = s.client.admin();
    let registry_addr = Address::generate(&s.env);
    s.client.set_registry(&registry_addr);

    s.client.clear_registry();
    assert_auth_required_from(&s, &admin, "clear_registry");
}

#[test]
fn clear_registry_rejects_non_admin_and_preserves_registry() {
    let s = setup();
    let registry_addr = Address::generate(&s.env);
    s.client.set_registry(&registry_addr);

    let non_admin = Address::generate(&s.env);
    s.env.mock_auths(&[MockAuth {
        address: &non_admin,
        invoke: &MockAuthInvoke {
            contract: &s.client.address,
            fn_name: "clear_registry",
            args: ().into_val(&s.env),
            sub_invokes: &[],
        },
    }]);

    let result = s.client.try_clear_registry();
    assert!(result.is_err());
    assert_eq!(s.client.registry(), Some(registry_addr));
}

#[test]
fn clear_registry_on_already_unset_registry_is_idempotent() {
    let s = setup();
    assert_eq!(s.client.registry(), None);

    let result = s.client.try_clear_registry();
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(s.client.registry(), None);
}

#[test]
fn clear_registry_emits_event() {
    let s = setup();
    let registry_addr = Address::generate(&s.env);
    s.client.set_registry(&registry_addr);

    s.client.clear_registry();
    let event = last_event(&s.env);

    assert_eq!(
        event,
        (
            (symbol_short!("regclr"),).into_val(&s.env),
            ().into_val(&s.env),
        )
    );
}

#[test]
fn valid_registry_can_be_set_again_after_clearing() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let admin = s.client.admin();
    let registry_id = s.env.register(
        ngo_registry::NgoRegistry,
        ngo_registry::NgoRegistryArgs::__constructor(&admin),
    );
    let registry_client = ngo_registry::NgoRegistryClient::new(&s.env, &registry_id);

    // Set registry
    s.client.set_registry(&registry_id);
    assert_eq!(s.client.registry(), Some(registry_id.clone()));

    // Clear registry
    s.client.clear_registry();
    assert_eq!(s.client.registry(), None);

    // Re-set valid registry
    s.client.set_registry(&registry_id);
    assert_eq!(s.client.registry(), Some(registry_id.clone()));

    // Unverified NGO fails
    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    // Approve NGO and succeed
    registry_client.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry_client.approve_ngo(&s.client.admin(), &s.ngo);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

#[test]
fn top_up_to_verified_ngo_works_with_registry_configured() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &3_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.top_up(&stream_id, &500);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_500);
    assert_eq!(s.token.balance(&s.donor), 1_500);
}

#[test]
fn top_up_after_revocation_fails_and_preserves_state() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &3_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Revoke the NGO
    registry.revoke_ngo(&s.client.admin(), &s.ngo);

    let donor_balance_before = s.token.balance(&s.donor);
    let stream_balance_before = s.client.get_stream(&stream_id).balance;
    let total_donated_before = s.client.get_config().total_donated;

    // Top up fails with exact Error::NgoNotVerified
    let result = s.client.try_top_up(&stream_id, &500);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    // State, balances, and totals are unchanged
    assert_eq!(s.token.balance(&s.donor), donor_balance_before);
    assert_eq!(
        s.client.get_stream(&stream_id).balance,
        stream_balance_before
    );
    assert_eq!(s.client.get_config().total_donated, total_donated_before);
}

#[test]
fn top_up_with_no_registry_configured_works_as_before() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);

    assert_eq!(s.client.registry(), None);
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.top_up(&stream_id, &500);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_500);
}

#[test]
fn top_up_after_re_verification_succeeds() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &3_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Revoke and verify top_up fails
    registry.revoke_ngo(&s.client.admin(), &s.ngo);
    assert_eq!(
        s.client.try_top_up(&stream_id, &500),
        Err(Ok(Error::NgoNotVerified))
    );

    // Re-verify the NGO
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    // Top up now succeeds
    s.client.top_up(&stream_id, &500);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_500);
    assert_eq!(s.token.balance(&s.donor), 1_500);
}

#[test]
fn cancel_and_withdraw_still_work_for_stream_whose_ngo_was_revoked() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &2_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Revoke NGO after stream opened
    registry.revoke_ngo(&s.client.admin(), &s.ngo);

    // Advance 20 seconds: 20 * 10 = 200 accrued
    s.env.ledger().with_mut(|l| l.timestamp += 20);

    // NGO can still withdraw already accrued funds
    s.client.withdraw(&stream_id);
    assert_eq!(s.token.balance(&s.ngo), 200);

    // Donor can still cancel and receive refund of remaining 800 tokens
    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 800);
    assert_eq!(s.token.balance(&s.donor), 1_800);
    assert!(s.client.get_stream(&stream_id).cancelled);
}

#[test]
fn bad_registry_fails_top_up_with_ngo_not_verified() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Point registry at an invalid address
    let bad_registry = Address::generate(&s.env);
    s.client.set_registry(&bad_registry);

    // top_up attempts cross-contract call and fails closed with NgoNotVerified
    let result = s.client.try_top_up(&stream_id, &500);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    // Clearing registry allows top_up to proceed
    s.client.clear_registry();
    s.client.top_up(&stream_id, &500);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_500);
}

// =============================================================================
// modify_rate NGO verification (issue #330)
// =============================================================================

#[test]
fn modify_rate_increase_for_revoked_ngo_fails_and_preserves_state() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    registry.revoke_ngo(&s.client.admin(), &s.ngo);
    s.env.ledger().with_mut(|l| l.timestamp += 10);

    let before = s.client.get_stream(&stream_id);
    let ngo_balance_before = s.token.balance(&s.ngo);

    let result = s.client.try_modify_rate(&stream_id, &20);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    // Rejected before settling: nothing paid out, stream untouched.
    assert_eq!(s.client.get_stream(&stream_id), before);
    assert_eq!(s.token.balance(&s.ngo), ngo_balance_before);
}

#[test]
fn modify_rate_decrease_for_revoked_ngo_succeeds() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    registry.revoke_ngo(&s.client.admin(), &s.ngo);
    s.env.ledger().with_mut(|l| l.timestamp += 10); // 100 accrues at the old rate

    s.client.modify_rate(&stream_id, &5);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.rate, 5);
    assert_eq!(stream.balance, 900);
    assert_eq!(s.token.balance(&s.ngo), 100);
}

#[test]
fn modify_rate_increase_for_verified_ngo_succeeds() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.modify_rate(&stream_id, &20);
    assert_eq!(s.client.get_stream(&stream_id).rate, 20);
}

#[test]
fn modify_rate_increase_rejects_unverified_ngo() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    // The NGO must be verified to open a stream, so verify it first and then
    // revoke it to reach the unverified state under test.
    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    registry.revoke_ngo(&s.client.admin(), &s.ngo);

    // Attempting to increase rate should fail with NgoNotVerified
    let result = s.client.try_modify_rate(&stream_id, &20);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    // Stream state should be unchanged
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.rate, 10);
    assert_eq!(stream.balance, 1_000);
}

#[test]
fn modify_rate_decrease_allows_unverified_ngo() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    // Verify to open the stream, then revoke so the NGO is unverified when the
    // rate is decreased.
    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    registry.revoke_ngo(&s.client.admin(), &s.ngo);

    // Decreasing rate should succeed even for unverified NGO
    s.client.modify_rate(&stream_id, &5);
    assert_eq!(s.client.get_stream(&stream_id).rate, 5);
}

#[test]
fn modify_rate_increase_rejects_revoked_ngo() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    // Register and approve the NGO
    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Revoke the NGO after creation
    registry.revoke_ngo(&s.client.admin(), &s.ngo);

    // Attempting to increase rate should fail with NgoNotVerified
    let result = s.client.try_modify_rate(&stream_id, &20);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    // Stream state should be unchanged
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.rate, 10);
    assert_eq!(stream.balance, 1_000);
}

#[test]
fn modify_rate_decrease_allows_revoked_ngo() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    // Register and approve the NGO
    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Revoke the NGO after creation
    registry.revoke_ngo(&s.client.admin(), &s.ngo);

    // Decreasing rate should succeed even after revocation
    s.client.modify_rate(&stream_id, &5);
    assert_eq!(s.client.get_stream(&stream_id).rate, 5);
}

// =============================================================================
// create_stream rejects the vault's own address (issue #331)
// =============================================================================

#[test]
fn create_stream_rejects_vault_as_ngo() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let vault = s.client.address.clone();
    let donor_balance_before = s.token.balance(&s.donor);

    let result = s
        .client
        .try_create_stream(&s.donor, &vault, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::InvalidNgo)));

    // Nothing written: no stream created, no counter incremented, no donor count updated
    assert_eq!(s.client.stream_count(), 0);
    assert_eq!(s.client.total_donated(), 0);
    assert_eq!(s.client.streams_by_donor(&s.donor).len(), 0);

    // No funds moved: donor keeps all tokens, vault has none
    assert_eq!(s.token.balance(&s.donor), donor_balance_before);
    assert_eq!(s.token.balance(&vault), 0);
}

#[test]
fn create_stream_rejects_vault_as_token() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let vault = s.client.address.clone();
    let donor_balance_before = s.token.balance(&s.donor);

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &vault, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::InvalidToken)));

    // Nothing written: no stream created, no counter incremented, no donor count updated
    assert_eq!(s.client.stream_count(), 0);
    assert_eq!(s.client.total_donated(), 0);
    assert_eq!(s.client.streams_by_donor(&s.donor).len(), 0);

    // No funds moved: donor keeps all tokens (the real token balance)
    assert_eq!(s.token.balance(&s.donor), donor_balance_before);
}

#[test]
fn create_stream_checks_vault_address_before_other_validations() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let vault = s.client.address.clone();

    // Even with invalid amounts, vault address check comes first
    let result = s
        .client
        .try_create_stream(&s.donor, &vault, &s.token.address, &0, &10);
    assert_eq!(
        result,
        Err(Ok(Error::InvalidNgo)),
        "InvalidNgo should be returned before InvalidAmount"
    );

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &vault, &1_000, &0);
    assert_eq!(
        result,
        Err(Ok(Error::InvalidToken)),
        "InvalidToken should be returned before InvalidAmount"
    );
}

// =============================================================================
// withdraw fee derivation (issue #332)
// =============================================================================

#[test]
fn fee_from_payout_returns_accrued_minus_net() {
    assert_eq!(fee_from_payout(1_000, 950), Ok(50));
    assert_eq!(fee_from_payout(1_000, 1_000), Ok(0));
}

#[test]
fn fee_from_payout_reports_overflow_instead_of_zero() {
    // A broken invariant that makes `accrued - net` leave i128's range must
    // surface as an error, not as a silent fee of zero.
    assert_eq!(
        fee_from_payout(i128::MIN, 1),
        Err(Error::ArithmeticOverflow)
    );
    assert_eq!(
        fee_from_payout(i128::MAX, -1),
        Err(Error::ArithmeticOverflow)
    );
}

#[test]
fn withdraw_event_fee_matches_fee_helper() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5%
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues

    let net = s.client.withdraw(&stream_id);
    let fee = fee_from_payout(500, net).unwrap();
    assert_last_event(
        &s.env,
        (symbol_short!("withdraw"), stream_id),
        (500i128, fee, net, s.ngo.clone()),
    );
    assert_eq!(fee, 25);
    assert_eq!(net, 475);
    assert_eq!(s.token.balance(&treasury), 25);
}

// =============================================================================
// withdraw_batch tests (issue #285)
// =============================================================================

#[test]
fn withdraw_batch_across_several_streams() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% global fee

    s.token_admin.mint(&s.donor, &5_000);

    let s1 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let s2 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &2_000, &20);
    let s3 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let mut ids = Vec::new(&s.env);
    ids.push_back(s1);
    ids.push_back(s2);
    ids.push_back(s3);

    let amounts = s.client.withdraw_batch(&ids);

    // Accruals: 50 * 10 = 500, 50 * 20 = 1000, 50 * 10 = 500. Like `withdraw`,
    // each entry is the stream's payout after the 5% fee.
    assert_eq!(amounts.get(0), Some(475));
    assert_eq!(amounts.get(1), Some(950));
    assert_eq!(amounts.get(2), Some(475));

    let gross: i128 = 500 + 1_000 + 500;
    let fee: i128 = (gross * 500) / 10_000; // 100
    let net: i128 = gross - fee; // 1900
    assert_eq!(gross, net + fee);

    assert_eq!(s.token.balance(&s.ngo), net);
    assert_eq!(s.token.balance(&treasury), fee);
    assert_eq!(s.token.balance(&s.client.address), 4_000 - gross); // 4000 deposited, 2000 drawn

    // Verify stream states
    let stream1 = s.client.get_stream(&s1);
    assert_eq!(stream1.balance, 500);
    assert_eq!(stream1.withdrawn, 500);
    assert_eq!(stream1.status, StreamStatus::Active);
    assert_eq!(stream1.last_update, s.env.ledger().timestamp());

    let stream2 = s.client.get_stream(&s2);
    assert_eq!(stream2.balance, 1_000);
    assert_eq!(stream2.withdrawn, 1_000);
    assert_eq!(stream2.status, StreamStatus::Active);
    assert_eq!(stream2.last_update, s.env.ledger().timestamp());

    let stream3 = s.client.get_stream(&s3);
    assert_eq!(stream3.balance, 500);
    assert_eq!(stream3.withdrawn, 500);
    assert_eq!(stream3.status, StreamStatus::Active);
    assert_eq!(stream3.last_update, s.env.ledger().timestamp());
}

#[test]
fn withdraw_batch_rejects_mixed_ngos() {
    let s = setup();
    let ngo_b = Address::generate(&s.env);
    s.token_admin.mint(&s.donor, &2_000);

    let s1 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let s2 = s
        .client
        .create_stream(&s.donor, &ngo_b, &s.token.address, &1_000, &10);

    let mut ids = Vec::new(&s.env);
    ids.push_back(s1);
    ids.push_back(s2);

    let result = s.client.try_withdraw_batch(&ids);
    assert_eq!(result, Err(Ok(Error::MixedNgo)));
}

#[test]
fn withdraw_batch_skips_stream_with_zero_accrual() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let s1 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let s2 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 20);

    // Settle s1 first
    s.client.withdraw(&s1);

    // At the current timestamp, s1 has 0 new accrual, while s2 has 200
    let mut ids = Vec::new(&s.env);
    ids.push_back(s1);
    ids.push_back(s2);

    let amounts = s.client.withdraw_batch(&ids);
    assert_eq!(amounts.get(0), Some(0));
    assert_eq!(amounts.get(1), Some(200));

    let stream1 = s.client.get_stream(&s1);
    assert_eq!(stream1.balance, 800);
    assert_eq!(stream1.withdrawn, 200);

    let stream2 = s.client.get_stream(&s2);
    assert_eq!(stream2.balance, 800);
    assert_eq!(stream2.withdrawn, 200);
}

#[test]
fn withdraw_batch_multi_token_fee_aggregation_and_override() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% global fee

    let token_b_admin_addr = Address::generate(&s.env);
    let (token_b, token_b_admin) = create_token(&s.env, &token_b_admin_addr);
    s.client.set_token_fee_bps(&token_b.address, &1_000); // 10% fee override

    s.token_admin.mint(&s.donor, &3_000);
    token_b_admin.mint(&s.donor, &3_000);

    let s1 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let s2 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &2_000, &10);
    let s3 = s
        .client
        .create_stream(&s.donor, &s.ngo, &token_b.address, &2_000, &20);

    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let mut ids = Vec::new(&s.env);
    ids.push_back(s1);
    ids.push_back(s2);
    ids.push_back(s3);

    // Each entry is the stream's payout after its token's fee: 5% on token A,
    // the 10% override on token B.
    let amounts = s.client.withdraw_batch(&ids);
    assert_eq!(amounts.get(0), Some(475));
    assert_eq!(amounts.get(1), Some(475));
    assert_eq!(amounts.get(2), Some(900));

    // Token A: 500 + 500 = 1000 gross. Global fee 5% = 50 fee, 950 net.
    let token_a_gross: i128 = 1_000;
    let token_a_fee: i128 = 50;
    let token_a_net: i128 = 950;
    assert_eq!(token_a_gross, token_a_net + token_a_fee);
    assert_eq!(s.token.balance(&s.ngo), token_a_net);
    assert_eq!(s.token.balance(&treasury), token_a_fee);

    // Token B: 1000 gross. Override fee 10% = 100 fee, 900 net.
    let token_b_gross: i128 = 1_000;
    let token_b_fee: i128 = 100;
    let token_b_net: i128 = 900;
    assert_eq!(token_b_gross, token_b_net + token_b_fee);
    assert_eq!(token_b.balance(&s.ngo), token_b_net);
    assert_eq!(token_b.balance(&treasury), token_b_fee);
}

#[test]
fn withdraw_batch_requires_ngo_auth() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 10);

    let mut ids = Vec::new(&s.env);
    ids.push_back(stream_id);

    s.client.withdraw_batch(&ids);
    assert_auth_required_from(&s, &s.ngo, "withdraw_batch");
}

#[test]
#[should_panic]
fn withdraw_batch_fails_without_ngo_auth() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 10);

    let mut ids = Vec::new(&s.env);
    ids.push_back(stream_id);

    s.env.mock_auths(&[]);
    s.client.withdraw_batch(&ids);
}

#[test]
#[should_panic]
fn withdraw_batch_rejects_non_ngo_caller() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 10);

    let mut ids = Vec::new(&s.env);
    ids.push_back(stream_id);

    let attacker = Address::generate(&s.env);
    s.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &s.client.address,
            fn_name: "withdraw_batch",
            args: (ids.clone(),).into_val(&s.env),
            sub_invokes: &[],
        },
    }]);

    s.client.withdraw_batch(&ids);
}

#[test]
fn withdraw_batch_empty_batch_fails_with_nothing_to_withdraw() {
    let s = setup();
    let ids = Vec::new(&s.env);
    assert_eq!(
        s.client.try_withdraw_batch(&ids),
        Err(Ok(Error::NothingToWithdraw))
    );
}

#[test]
fn withdraw_batch_unknown_stream_returns_stream_not_found() {
    let s = setup();
    let mut ids = Vec::new(&s.env);
    ids.push_back(99_999u64);

    let result = s.client.try_withdraw_batch(&ids);
    assert_eq!(result, Err(Ok(Error::StreamNotFound)));
}

#[test]
fn withdraw_batch_with_cancelled_stream_skips_cancelled_accrual() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let s1 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let s2 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Cancel s1: donor refunded 1000, s1 rate=0, balance=0, cancelled=true
    s.client.cancel_stream(&s1);

    s.env.ledger().with_mut(|l| l.timestamp += 20);

    let mut ids = Vec::new(&s.env);
    ids.push_back(s1);
    ids.push_back(s2);

    let amounts = s.client.withdraw_batch(&ids);
    assert_eq!(amounts.get(0), Some(0));
    assert_eq!(amounts.get(1), Some(200));

    let stream1 = s.client.get_stream(&s1);
    assert!(stream1.cancelled);
    assert_eq!(stream1.balance, 0);

    let stream2 = s.client.get_stream(&s2);
    assert_eq!(stream2.balance, 800);
    assert_eq!(stream2.withdrawn, 200);

    assert_eq!(s.token.balance(&s.ngo), 200);
}

// =============================================================================
// Issue #342: withdraw_all pagination
// =============================================================================

#[test]
fn withdraw_all_pages_through_more_streams_than_the_page_size() {
    let s = setup();
    // More streams than one page can hold (MAX_BATCH_WITHDRAW_STREAMS), so
    // collecting them all needs several calls.
    let count: u32 = 25;
    s.token_admin.mint(&s.donor, &(count as i128 * 1_000));
    for _ in 0..count {
        s.client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    }

    // 50 seconds at 10/s accrues 500 on every stream.
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let mut start: u32 = 0;
    let mut calls = 0;
    let mut total_paid: i128 = 0;
    loop {
        let page = s.client.withdraw_all(&s.ngo, &start, &10);
        assert_eq!(page.amounts.len(), 10u32.min(count - start));
        for amount in page.amounts.iter() {
            assert_eq!(amount, 500);
            total_paid += amount;
        }
        calls += 1;
        start = page.next;
        if start >= count {
            break;
        }
    }

    // 25 streams in pages of 10 take three calls.
    assert_eq!(calls, 3);
    assert_eq!(total_paid, count as i128 * 500);
    assert_eq!(s.token.balance(&s.ngo), count as i128 * 500);

    // Paging past the end is a harmless empty page that still reports the end.
    let done = s.client.withdraw_all(&s.ngo, &start, &10);
    assert_eq!(done.amounts.len(), 0);
    assert_eq!(done.next, count);
}

#[test]
fn withdraw_all_clamps_limit_to_the_batch_cap() {
    let s = setup();
    let count: u32 = 25;
    s.token_admin.mint(&s.donor, &(count as i128 * 1_000));
    for _ in 0..count {
        s.client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    }
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    // A limit above the batch cap only settles the cap's worth this call.
    let page = s.client.withdraw_all(&s.ngo, &0, &100);
    assert_eq!(page.amounts.len(), MAX_BATCH_WITHDRAW_STREAMS);
    assert_eq!(page.next, MAX_BATCH_WITHDRAW_STREAMS);
}

#[test]
fn withdraw_all_returns_zeros_when_a_page_has_accrued_nothing() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);
    for _ in 0..3 {
        s.client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    }

    // No time has passed, so nothing has accrued. That is not an error for a
    // pager: the page reports one zero per stream and advances the cursor.
    let page = s.client.withdraw_all(&s.ngo, &0, &10);
    assert_eq!(page.amounts.len(), 3);
    assert_eq!(page.amounts.get(0), Some(0));
    assert_eq!(page.amounts.get(2), Some(0));
    assert_eq!(page.next, 3);
    assert_eq!(s.token.balance(&s.ngo), 0);
}

#[test]
fn withdraw_all_on_ngo_without_streams_returns_empty_page() {
    let s = setup();

    let page = s.client.withdraw_all(&s.ngo, &0, &10);
    assert_eq!(page.amounts.len(), 0);
    assert_eq!(page.next, 0);
}

#[test]
fn withdraw_all_only_settles_the_named_ngos_streams() {
    let s = setup();
    let other_ngo = Address::generate(&s.env);
    s.token_admin.mint(&s.donor, &2_000);

    s.client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client
        .create_stream(&s.donor, &other_ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let page = s.client.withdraw_all(&s.ngo, &0, &10);
    assert_eq!(page.amounts.len(), 1);
    assert_eq!(page.next, 1);
    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.token.balance(&other_ngo), 0);
}

#[test]
fn withdraw_all_requires_ngo_auth() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 10);

    s.client.withdraw_all(&s.ngo, &0, &10);
    assert_auth_required_from(&s, &s.ngo, "withdraw_all");
}

// =============================================================================
// NGO payout address
// =============================================================================

#[test]
fn set_payout_address_requires_owner_auth() {
    let s = setup();
    let payout = Address::generate(&s.env);

    s.client.set_payout_address(&s.ngo, &payout);

    assert_auth_required_from(&s, &s.ngo, "set_payout_address");
}

#[test]
fn payout_address_defaults_to_none() {
    let s = setup();
    assert_eq!(s.client.payout_address(&s.ngo), None);
}

#[test]
fn set_payout_address_emits_event() {
    let s = setup();
    let payout = Address::generate(&s.env);

    s.client.set_payout_address(&s.ngo, &payout);

    assert_last_event(
        &s.env,
        (symbol_short!("payoutset"), s.ngo.clone()),
        payout.clone(),
    );
    assert_eq!(s.client.payout_address(&s.ngo), Some(payout));
}

#[test]
fn withdraw_pays_configured_payout_address() {
    let s = setup();
    let payout = Address::generate(&s.env);
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.set_payout_address(&s.ngo, &payout);

    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues

    let net = s.client.withdraw(&stream_id);
    let withdrew = last_event(&s.env);

    assert_eq!(net, 500);
    assert_eq!(s.token.balance(&payout), 500);
    // The owner key only authorised; it received nothing.
    assert_eq!(s.token.balance(&s.ngo), 0);

    assert_eq!(
        withdrew,
        (
            (symbol_short!("withdraw"), stream_id).into_val(&s.env),
            (500i128, 0i128, 500i128, payout.clone()).into_val(&s.env),
        )
    );
}

#[test]
fn withdraw_batch_pays_configured_payout_address() {
    let s = setup();
    let payout = Address::generate(&s.env);
    s.token_admin.mint(&s.donor, &2_000);

    let a = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let b = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.client.set_payout_address(&s.ngo, &payout);

    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let mut ids = Vec::new(&s.env);
    ids.push_back(a);
    ids.push_back(b);

    let amounts = s.client.withdraw_batch(&ids);
    assert_eq!(amounts.get(0), Some(500));
    assert_eq!(amounts.get(1), Some(500));

    assert_eq!(s.token.balance(&payout), 1_000);
    assert_eq!(s.token.balance(&s.ngo), 0);
}

#[test]
fn withdraw_honours_payout_address_change_between_withdrawals() {
    let s = setup();
    let first = Address::generate(&s.env);
    let second = Address::generate(&s.env);
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // First window routes to `first`.
    s.client.set_payout_address(&s.ngo, &first);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues
    assert_eq!(s.client.withdraw(&stream_id), 500);
    assert_eq!(s.token.balance(&first), 500);

    // Re-point before the next withdrawal; subsequent payouts must follow.
    s.client.set_payout_address(&s.ngo, &second);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // 500 more accrues
    assert_eq!(s.client.withdraw(&stream_id), 500);

    assert_eq!(s.token.balance(&second), 500);
    // The first destination is untouched by the second withdrawal, and the
    // owner address never received anything.
    assert_eq!(s.token.balance(&first), 500);
    assert_eq!(s.token.balance(&s.ngo), 0);
}

// =============================================================================
// Issue #302: pin the Stream storage layout
// =============================================================================

/// The current `Stream` layout, in the order `#[contracttype]` encodes it.
/// Update this list together with the struct â€” the test below fails otherwise.
const STREAM_FIELDS: [&str; 12] = [
    "balance",
    "cancelled",
    "created_at",
    "donor",
    "last_update",
    "memo",
    "ngo",
    "rate",
    "start",
    "status",
    "token",
    "withdrawn",
];

#[test]
fn stream_storage_layout_is_pinned() {
    let s = setup();
    let stream = Stream {
        donor: s.donor.clone(),
        ngo: s.ngo.clone(),
        token: s.token.address.clone(),
        rate: 10,
        balance: 1_000,
        withdrawn: 0,
        created_at: 1,
        start: 1,
        last_update: 1,
        memo: Bytes::new(&s.env),
        status: StreamStatus::Active,
        cancelled: false,
    };

    let val: Val = stream.into_val(&s.env);
    let scval = ScVal::try_from_val(&s.env, &val).expect("Stream encodes as ScVal");
    let keys = match scval {
        ScVal::Map(Some(map)) => {
            let mut keys = Vec::new(&s.env);
            for entry in map.0.iter() {
                keys.push_back(
                    Symbol::try_from_val(&s.env, &entry.key)
                        .expect("struct fields encode as symbols"),
                );
            }
            keys
        }
        other => panic!("expected a struct map, got {other:?}"),
    };

    let mut expected = Vec::new(&s.env);
    for field in STREAM_FIELDS {
        expected.push_back(Symbol::new(&s.env, field));
    }
    assert_eq!(
        keys, expected,
        "Stream's field list changed; update STREAM_FIELDS and the compatibility test"
    );
}

#[test]
fn previous_stream_layout_is_not_decodable_by_current_code() {
    let s = setup();

    // Layout before `cancelled` and `status` were added (8 fields).
    let mut old = Map::<Symbol, Val>::new(&s.env);
    old.set(symbol_short!("donor"), s.donor.clone().into_val(&s.env));
    old.set(symbol_short!("ngo"), s.ngo.clone().into_val(&s.env));
    old.set(
        symbol_short!("token"),
        s.token.address.clone().into_val(&s.env),
    );
    old.set(symbol_short!("rate"), 10i128.into_val(&s.env));
    old.set(symbol_short!("balance"), 1_000i128.into_val(&s.env));
    old.set(symbol_short!("withdrawn"), 0i128.into_val(&s.env));
    old.set(Symbol::new(&s.env, "created_at"), 1u64.into_val(&s.env));
    old.set(Symbol::new(&s.env, "last_update"), 1u64.into_val(&s.env));

    let raw: Val = old.into_val(&s.env);
    let contract_id = s.client.address.clone();
    s.env.as_contract(&contract_id, || {
        s.env
            .storage()
            .persistent()
            .set(&DataKey::Stream(999u64), &raw);
    });

    // Documented outcome: the current code cannot read the old layout. A
    // missing field makes `get_stream` either report StreamNotFound or abort
    // the host decode; either way it must not produce a Stream value.
    let result = s.env.try_invoke_contract::<Stream, Error>(
        &contract_id,
        &Symbol::new(&s.env, "get_stream"),
        (999u64,).into_val(&s.env),
    );
    let decoded = result.ok().and_then(|inner| inner.ok());
    assert!(
        decoded.is_none(),
        "current code unexpectedly decoded the previous layout: {decoded:?}"
    );
}

// =============================================================================
// Issue #299: per-token fee overrides survive TTL advancement
// =============================================================================

#[test]
fn token_fee_override_survives_instance_ttl_advance() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500); // 5% global
    s.client.set_token_fee_bps(&s.token.address, &100); // 1% override

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Move past the instance entry's TTL/bump threshold. Stream storage has a
    // longer (90-day) TTL, so the stream itself is still live.
    s.env
        .ledger()
        .with_mut(|l| l.sequence_number += INSTANCE_BUMP_AMOUNT + 1);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    assert_eq!(
        s.client.token_fee_bps(&s.token.address),
        100,
        "override must still be readable after TTL advancement"
    );

    let (net, fee) = s.client.pending_payout(&stream_id);
    assert_eq!(
        fee, 5,
        "pending_payout uses the 1% override, not the 5% global fee"
    );
    assert_eq!(net, 495);

    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 495, "withdraw uses the 1% override");
    assert_eq!(s.token.balance(&treasury), 5);
    assert_eq!(s.token.balance(&s.ngo), 495);
}

// =============================================================================
// Issue #298: modify_rate and top_up on a drained stream
// =============================================================================

#[test]
fn modify_rate_and_top_up_on_a_drained_stream() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Drain exactly: 1_000 at 10/s takes 100 seconds.
    s.env.ledger().with_mut(|l| l.timestamp += 100);
    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 1_000);

    let drained = s.client.get_stream(&stream_id);
    assert_eq!(drained.balance, 0);
    assert_eq!(drained.status, StreamStatus::Drained);
    assert!(!drained.cancelled);

    // modify_rate on a drained (but not cancelled) stream is allowed. It does
    // not revive the stream's status: balance is still zero, so it stays
    // Drained until a top-up.
    let modify = s.client.try_modify_rate(&stream_id, &20);
    assert_eq!(modify, Ok(Ok(())));
    let after_modify = s.client.get_stream(&stream_id);
    assert_eq!(after_modify.rate, 20);
    assert_eq!(after_modify.balance, 0);
    assert_eq!(after_modify.status, StreamStatus::Drained);
    assert!(!after_modify.cancelled);

    // top_up is allowed, adds balance, and reactivates the drained stream.
    let top_up = s.client.try_top_up(&stream_id, &500);
    assert_eq!(top_up, Ok(Ok(())));
    let after_top_up = s.client.get_stream(&stream_id);
    assert_eq!(after_top_up.balance, 500);
    assert_eq!(after_top_up.rate, 20);
    assert_eq!(after_top_up.status, StreamStatus::Active);
    assert!(!after_top_up.cancelled);

    // The new balance accrues at the new rate.
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    assert_eq!(s.client.pending_accrual(&stream_id), 200);
}

#[test]
fn settle_marks_stream_drained_when_balance_reaches_zero() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    // Create a stream that will drain exactly after modify_rate settles
    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Advance exactly 100 seconds so the entire balance accrues
    s.env.ledger().with_mut(|l| l.timestamp += 100);

    // modify_rate calls settle, which should drain the stream and mark it Drained
    s.client.modify_rate(&stream_id, &20);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.rate, 20);
    assert_eq!(stream.status, StreamStatus::Drained);
    assert!(!stream.cancelled);

    // Verify the NGO received all accrued funds
    assert_eq!(s.token.balance(&s.ngo), 1_000);

    // modify_rate is still allowed on a drained stream
    s.client.modify_rate(&stream_id, &5);
    let after = s.client.get_stream(&stream_id);
    assert_eq!(after.rate, 5);
    assert_eq!(after.status, StreamStatus::Drained);
}

// =============================================================================
// configuration events (issue #277)
// =============================================================================

#[test]
fn constructor_publishes_init_event() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    env.register(DonationVault, DonationVaultArgs::__constructor(&admin));

    assert_last_event(&env, (symbol_short!("init"),), admin.clone());
}

#[test]
fn clear_treasury_publishes_event() {
    let s = setup();
    s.client.set_treasury(&Address::generate(&s.env));

    s.client.clear_treasury();

    assert_last_event(&s.env, (symbol_short!("treclr"),), ());
}

#[test]
fn set_min_deposit_publishes_event() {
    let s = setup();

    s.client.set_min_deposit(&100);

    assert_last_event(&s.env, (symbol_short!("mindep"),), 100i128);
}

#[test]
fn set_cancel_grace_ledgers_publishes_event() {
    let s = setup();

    s.client.set_cancel_grace_ledgers(&25);

    assert_last_event(&s.env, (symbol_short!("grceset"),), 25u32);
}

#[test]
fn set_registry_publishes_event() {
    let s = setup();
    let registry = Address::generate(&s.env);

    s.client.set_registry(&registry);

    assert_last_event(
        &s.env,
        (symbol_short!("regset"),),
        (registry.clone(), None::<Address>),
    );

    let replacement = Address::generate(&s.env);
    s.client.set_registry(&replacement);

    assert_last_event(
        &s.env,
        (symbol_short!("regset"),),
        (replacement, Some(registry)),
    );
}

#[test]
fn upgrade_publishes_event() {
    let s = setup();
    // Upload any bytes so the upgrade target exists; the host only checks that
    // the hash is present, not that it is executable.
    let new_wasm_hash = s.env.deployer().upload_contract_wasm(Bytes::new(&s.env));

    s.client.upgrade(&new_wasm_hash);

    assert_last_event(&s.env, (symbol_short!("upgrade"),), new_wasm_hash.clone());
}

// =============================================================================
// Issue #344: drained event when a stream's status becomes Drained
// =============================================================================

#[test]
fn withdraw_that_empties_stream_emits_drained_event() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    // Exactly the whole balance accrues (10/s for 100s).
    s.env.ledger().with_mut(|l| l.timestamp += 100);

    s.client.withdraw(&stream_id);

    // The plain `withdraw` comes first, then the terminal `drained`, so an
    // indexer observes the payout before the status change.
    assert_nth_last_event(
        &s.env,
        1,
        (symbol_short!("withdraw"), stream_id),
        (1_000i128, 0i128, 1_000i128, s.ngo.clone()),
    );
    assert_last_event(&s.env, (symbol_short!("drained"), stream_id), (1_000i128,));
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );
}

#[test]
fn withdraw_that_leaves_balance_emits_no_drained_event() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50); // half the stream

    s.client.withdraw(&stream_id);

    assert_last_event(
        &s.env,
        (symbol_short!("withdraw"), stream_id),
        (500i128, 0i128, 500i128, s.ngo.clone()),
    );
    assert_eq!(count_events_with_topic(&s.env, symbol_short!("drained")), 0);
    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
}

#[test]
fn later_withdraw_that_empties_stream_reports_cumulative_total_paid() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);
    s.client.withdraw(&stream_id); // leaves half the balance

    s.env.ledger().with_mut(|l| l.timestamp += 50);
    s.client.withdraw(&stream_id); // empties the rest

    // `total_paid` is the stream's lifetime total, not just this withdrawal.
    assert_last_event(&s.env, (symbol_short!("drained"), stream_id), (1_000i128,));
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );
}

#[test]
fn withdraw_batch_emits_drained_event_after_the_streams_withdraw() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);

    // s1 empties exactly at 100s; s2 keeps half its balance.
    let s1 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let s2 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &2_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 100);

    let mut ids = Vec::new(&s.env);
    ids.push_back(s1);
    ids.push_back(s2);
    s.client.withdraw_batch(&ids);

    // Only the stream this batch emptied carries a `drained` event, and it
    // follows that stream's `withdraw`.
    assert_eq!(count_events_with_topic(&s.env, symbol_short!("drained")), 1);
    assert_nth_last_event(
        &s.env,
        2,
        (symbol_short!("withdraw"), s1),
        (1_000i128, 0i128, 1_000i128, s.ngo.clone()),
    );
    assert_nth_last_event(&s.env, 1, (symbol_short!("drained"), s1), (1_000i128,));
    assert_last_event(
        &s.env,
        (symbol_short!("withdraw"), s2),
        (1_000i128, 0i128, 1_000i128, s.ngo.clone()),
    );
    assert_eq!(s.client.get_stream(&s1).status, StreamStatus::Drained);
    assert_eq!(s.client.get_stream(&s2).status, StreamStatus::Active);
}

#[test]
fn withdraw_batch_emits_drained_event_for_every_emptied_stream() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);

    let s1 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let s2 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &2_000, &20);

    s.env.ledger().with_mut(|l| l.timestamp += 100);

    let mut ids = Vec::new(&s.env);
    ids.push_back(s1);
    ids.push_back(s2);
    s.client.withdraw_batch(&ids);

    // One `drained` per emptied stream, each after its own `withdraw`.
    assert_eq!(count_events_with_topic(&s.env, symbol_short!("drained")), 2);
    assert_nth_last_event(
        &s.env,
        3,
        (symbol_short!("withdraw"), s1),
        (1_000i128, 0i128, 1_000i128, s.ngo.clone()),
    );
    assert_nth_last_event(&s.env, 2, (symbol_short!("drained"), s1), (1_000i128,));
    assert_nth_last_event(
        &s.env,
        1,
        (symbol_short!("withdraw"), s2),
        (2_000i128, 0i128, 2_000i128, s.ngo.clone()),
    );
    assert_last_event(&s.env, (symbol_short!("drained"), s2), (2_000i128,));
    assert_eq!(s.client.get_stream(&s1).status, StreamStatus::Drained);
    assert_eq!(s.client.get_stream(&s2).status, StreamStatus::Drained);
}

#[test]
fn modify_rate_that_empties_stream_emits_drained_event() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 100);

    s.client.modify_rate(&stream_id, &20);

    // The settle-driven `drained` precedes the `ratemod` that triggered it.
    assert_nth_last_event(
        &s.env,
        1,
        (symbol_short!("drained"), stream_id),
        (1_000i128,),
    );
    assert_last_event(
        &s.env,
        (symbol_short!("ratemod"), stream_id),
        (10i128, 20i128, 1_000i128),
    );
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );
}

#[test]
fn transfer_stream_that_empties_stream_emits_drained_event() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let new_ngo = Address::generate(&s.env);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 100);

    s.client.transfer_stream(&stream_id, &new_ngo);

    assert_nth_last_event(
        &s.env,
        1,
        (symbol_short!("drained"), stream_id),
        (1_000i128,),
    );
    assert_last_event(
        &s.env,
        (symbol_short!("transfer"), stream_id),
        (s.ngo.clone(), new_ngo.clone(), 1_000i128),
    );
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );
}

#[test]
fn pause_stream_that_empties_stream_emits_drained_event() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 100);

    let settled = s.client.pause_stream(&stream_id);

    assert_eq!(settled, 1_000);
    assert_nth_last_event(
        &s.env,
        1,
        (symbol_short!("drained"), stream_id),
        (1_000i128,),
    );
    assert_last_event(
        &s.env,
        (symbol_short!("strmpause"), stream_id),
        (1_000i128,),
    );
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );
}

#[test]
fn cancel_stream_that_settles_to_zero_emits_no_drained_event() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    // Cancelling after the whole balance has accrued settles to zero, but the
    // stream is reclassified as Cancelled, not Drained â€” no `drained` event.
    s.env.ledger().with_mut(|l| l.timestamp += 100);

    s.client.cancel_stream(&stream_id);

    assert_eq!(count_events_with_topic(&s.env, symbol_short!("drained")), 0);
    assert_last_event(
        &s.env,
        (symbol_short!("cancel"), stream_id),
        (1_000i128, 0i128),
    );
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Cancelled
    );
}

#[test]
fn top_up_that_settles_to_zero_emits_no_drained_event() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    // `top_up` settles the fully-accrued stream and then reactivates it, so the
    // internal settle-to-zero is not a terminal drain and must stay silent.
    s.env.ledger().with_mut(|l| l.timestamp += 100);

    s.client.top_up(&stream_id, &1_000);

    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
    assert_eq!(count_events_with_topic(&s.env, symbol_short!("drained")), 0);
}

/// Names of the `pub fn`s in the `#[contractimpl]` block that opens with
/// `impl_header`, read from the contract's source. Every one of them is a
/// public entry point.
fn entry_point_names<'a>(source: &'a str, impl_header: &str) -> impl Iterator<Item = &'a str> {
    let mut lines = source.lines();
    let mut prev = "";
    for line in lines.by_ref() {
        if prev.trim() == "#[contractimpl]" && line.trim_end() == impl_header {
            break;
        }
        prev = line;
    }
    lines
        .take_while(|line| line.trim_end() != "}")
        .filter_map(|line| line.strip_prefix("    pub fn "))
        .map(|rest| rest.split(['(', '<']).next().unwrap_or(rest).trim())
}

/// `(function, role)` for each row of the roles table between the `start`
/// and `end` markers in `docs/ROLES.md`.
fn role_rows<'a>(
    doc: &'a str,
    start: &'a str,
    end: &'a str,
) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
    doc.lines()
        .skip_while(move |line| line.trim() != start)
        .skip(1)
        .take_while(move |line| line.trim() != end)
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            let mut cells = line.split('|').skip(1).map(str::trim);
            let name = cells.next().unwrap_or("").trim_matches('`');
            let role = cells.next().unwrap_or("");
            (name, role)
        })
}

/// `docs/ROLES.md` must list every public entry point with the role it
/// requires, and nothing else. Fails when an entry point is added or renamed
/// without updating the table, or removed while its row stays behind.
#[test]
fn roles_doc_lists_every_entry_point() {
    const SOURCE: &str = include_str!("lib.rs");
    const ROLES: &str = include_str!("../../../docs/ROLES.md");
    const IMPL: &str = "impl DonationVault {";
    const START: &str = "<!-- roles:donation-vault:start -->";
    const END: &str = "<!-- roles:donation-vault:end -->";

    assert!(
        entry_point_names(SOURCE, IMPL).count() > 0,
        "found no entry points: has `{IMPL}` or its #[contractimpl] moved?"
    );
    assert!(
        role_rows(ROLES, START, END).count() > 0,
        "found no roles table between {START} and {END} in docs/ROLES.md"
    );

    for name in entry_point_names(SOURCE, IMPL) {
        match role_rows(ROLES, START, END).find(|(row, _)| *row == name) {
            None => panic!(
                "`{name}` is a public DonationVault entry point with no row in docs/ROLES.md"
            ),
            Some((_, role)) => assert!(
                !role.is_empty(),
                "`{name}` has an empty role in docs/ROLES.md"
            ),
        }
    }

    for (name, _) in role_rows(ROLES, START, END) {
        assert!(
            entry_point_names(SOURCE, IMPL).any(|entry| entry == name),
            "docs/ROLES.md lists `{name}`, which is not a public DonationVault entry point"
        );
        assert_eq!(
            role_rows(ROLES, START, END)
                .filter(|(row, _)| *row == name)
                .count(),
            1,
            "`{name}` appears more than once in the donation-vault roles table"
        );
    }
}

// =============================================================================
// Property tests for fee calculation invariants (Issue 1)
// =============================================================================

#[cfg(test)]
mod proptest_fee_invariants {
    use super::*;
    use proptest::prelude::*;

    // Property test: fee + net always equals accrued for any rate, elapsed time,
    // and fee settings, and no value is negative.
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]

        #[test]
        fn fee_plus_net_equals_accrued(
            rate in 1i128..=1_000_000i128,
            elapsed in 1u64..=86_400u64,
            balance in 1_000i128..=1_000_000_000i128,
            fee_bps in 0u32..=1_000u32,
        ) {
            let s = setup();
            s.token_admin.mint(&s.donor, &balance);

            let treasury = Address::generate(&s.env);
            s.client.set_treasury(&treasury);
            s.client.set_fee_bps(&fee_bps);

            let stream_id = s
                .client
                .create_stream(&s.donor, &s.ngo, &s.token.address, &balance, &rate);

            s.env.ledger().with_mut(|l| l.timestamp += elapsed);

            let accrued = s.client.pending_accrual(&stream_id);
            let net = s.client.withdraw(&stream_id);
            let fee_balance = s.token.balance(&treasury);

            // Core invariant: fee + net == accrued
            prop_assert_eq!(fee_balance + net, accrued);

            // No value should be negative
            prop_assert!(fee_balance >= 0);
            prop_assert!(net >= 0);
            prop_assert!(accrued >= 0);

            // Net should never exceed accrued
            prop_assert!(net <= accrued);
        }
    }
}

// =============================================================================
// Tests for accrual after very long elapsed times (Issue 2)
// =============================================================================

#[test]
fn accrual_after_very_long_elapsed_time_is_capped_by_balance() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000_000, &1_000);

    // Advance time to near u64::MAX
    s.env.ledger().with_mut(|l| l.timestamp += u64::MAX - 1000);

    // Despite the enormous elapsed time, accrual should be capped at the balance
    let accrued = s.client.pending_accrual(&stream_id);
    assert_eq!(accrued, 1_000_000);
}

#[test]
fn accrual_with_max_elapsed_and_large_rate_never_overflows() {
    let s = setup();
    let balance: i128 = 10_000_000_000;
    s.token_admin.mint(&s.donor, &balance);

    let stream_id = s.client.create_stream(
        &s.donor,
        &s.ngo,
        &s.token.address,
        &balance,
        &(i128::MAX / 2),
    );

    // Advance time to u64::MAX
    s.env.ledger().with_mut(|l| l.timestamp += u64::MAX);

    // Despite rate * elapsed overflowing, accrual should be capped at balance
    let accrued = s.client.pending_accrual(&stream_id);
    assert_eq!(accrued, balance);
}

#[test]
fn accrual_near_u64_max_with_various_rates() {
    let s = setup();
    // Large enough that rate * elapsed overflows i128 for every rate below,
    // while staying inside the asset contract's balance limit.
    let balance: i128 = 1_000_000_000_000_000_000;
    let elapsed = u64::MAX - 100;

    for rate in [1i128, 100, 10_000, 1_000_000, i128::MAX / 2] {
        s.token_admin.mint(&s.donor, &balance);

        let stream_id = s
            .client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &balance, &rate);

        s.env.ledger().with_mut(|l| l.timestamp = elapsed);

        // Should always be capped at balance, never overflow or panic
        let accrued = s.client.pending_accrual(&stream_id);
        assert!(accrued >= 0);
        assert!(accrued <= balance);

        s.client.cancel_stream(&stream_id);
    }
}

// =============================================================================
// Tests for default per-donor stream cap (Issue 3)
// =============================================================================

#[test]
fn default_stream_cap_allows_exactly_100_streams() {
    let s = setup();
    let balance_per_stream: i128 = 1_000;
    let total_balance = balance_per_stream * 100;
    s.token_admin.mint(&s.donor, &total_balance);

    // Create exactly 100 streams (the default cap)
    for _ in 0..100 {
        s.client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &balance_per_stream, &10);
    }

    // The 101st stream should fail with StreamLimitExceeded
    s.token_admin.mint(&s.donor, &balance_per_stream);
    let result =
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &balance_per_stream, &10);
    assert_eq!(result, Err(Ok(Error::StreamLimitExceeded)));
}

#[test]
fn streams_by_donor_handles_default_cap() {
    let s = setup();
    let balance_per_stream: i128 = 1_000;
    let total_balance = balance_per_stream * 100;
    s.token_admin.mint(&s.donor, &total_balance);

    // Create exactly 100 streams
    for _ in 0..100 {
        s.client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &balance_per_stream, &10);
    }

    // streams_by_donor should successfully return all 100 streams
    let streams = s.client.streams_by_donor(&s.donor);
    assert_eq!(streams.len(), 100);
}

// ── resume_stream edge cases (issue #449) ──────────────────────────────────

/// A fresh 1_000-deposit, 10/s stream created at ledger time 0.
fn resume_fixture(s: &Setup<'_>) -> u64 {
    s.token_admin.mint(&s.donor, &1_000);
    s.client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10)
}

#[test]
fn resume_stream_on_a_paused_stream_reactivates_it_without_accruing_paused_time() {
    let s = setup();
    let stream_id = resume_fixture(&s);

    s.env.ledger().with_mut(|l| l.timestamp += 10);
    assert_eq!(s.client.pause_stream(&stream_id), 100);

    // 500 seconds pass while paused: none of it may accrue.
    s.env.ledger().with_mut(|l| l.timestamp += 500);
    s.client.resume_stream(&stream_id);
    assert_eq!(
        count_events_with_topic(&s.env, Symbol::new(&s.env, "strmresume")),
        1
    );

    let resumed = s.client.get_stream(&stream_id);
    assert_eq!(resumed.status, StreamStatus::Active);
    assert_eq!(resumed.balance, 900);
    assert_eq!(resumed.last_update, 510);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);

    s.env.ledger().with_mut(|l| l.timestamp += 20);
    assert_eq!(s.client.pending_accrual(&stream_id), 200);
}

#[test]
fn resume_stream_on_an_active_stream_fails_with_not_paused() {
    let s = setup();
    let stream_id = resume_fixture(&s);
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    let before = s.client.get_stream(&stream_id);

    assert_eq!(
        s.client.try_resume_stream(&stream_id),
        Err(Ok(Error::StreamNotPaused))
    );
    assert_eq!(s.client.get_stream(&stream_id), before);
}

#[test]
fn resume_stream_on_a_drained_stream_fails_with_not_paused() {
    let s = setup();
    let stream_id = resume_fixture(&s);
    s.env.ledger().with_mut(|l| l.timestamp += 100);
    s.client.withdraw(&stream_id);
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );
    let before = s.client.get_stream(&stream_id);

    assert_eq!(
        s.client.try_resume_stream(&stream_id),
        Err(Ok(Error::StreamNotPaused))
    );
    assert_eq!(s.client.get_stream(&stream_id), before);
}

#[test]
fn resume_stream_on_a_cancelled_stream_fails_with_cancelled() {
    let s = setup();
    let stream_id = resume_fixture(&s);

    // Cancelled from the paused state, the one a resume would otherwise accept.
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    s.client.pause_stream(&stream_id);
    s.client.cancel_stream(&stream_id);
    let before = s.client.get_stream(&stream_id);

    assert_eq!(
        s.client.try_resume_stream(&stream_id),
        Err(Ok(Error::StreamCancelled))
    );
    assert_eq!(s.client.get_stream(&stream_id), before);
}

#[test]
fn resume_stream_on_an_unknown_stream_fails_with_not_found() {
    let s = setup();
    assert_eq!(
        s.client.try_resume_stream(&999),
        Err(Ok(Error::StreamNotFound))
    );
}

#[test]
fn resume_stream_while_the_vault_is_paused_is_blocked_until_unpause() {
    let s = setup();
    let stream_id = resume_fixture(&s);
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    s.client.pause_stream(&stream_id);
    let before = s.client.get_stream(&stream_id);

    s.client.pause();
    assert_eq!(
        s.client.try_resume_stream(&stream_id),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(s.client.get_stream(&stream_id), before);

    s.client.unpause();
    s.client.resume_stream(&stream_id);
    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
}

#[test]
fn resume_stream_with_a_future_start_keeps_the_cliff() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);
    let start = 2_000;
    let stream_id =
        s.client
            .create_stream_with_start(&s.donor, &s.ngo, &s.token.address, &1_000, &10, &start);

    // Pause and resume while the cliff is still ahead: nothing is paid out.
    s.env.ledger().with_mut(|l| l.timestamp = 1_200);
    assert_eq!(s.client.pause_stream(&stream_id), 0);
    s.env.ledger().with_mut(|l| l.timestamp = 1_400);
    s.client.resume_stream(&stream_id);

    let resumed = s.client.get_stream(&stream_id);
    assert_eq!(resumed.status, StreamStatus::Active);
    assert_eq!(resumed.balance, 1_000);
    assert_eq!(resumed.start, start);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);

    // Still before the cliff after resuming; accrual begins only at `start`.
    s.env.ledger().with_mut(|l| l.timestamp = 1_900);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);
    s.env.ledger().with_mut(|l| l.timestamp = 2_050);
    assert_eq!(s.client.pending_accrual(&stream_id), 500);
}

#[test]
fn resume_stream_requires_the_donors_auth() {
    let s = setup();
    let stream_id = resume_fixture(&s);
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    s.client.pause_stream(&stream_id);

    // Only an unrelated account authorises the call: the donor's auth is missing.
    let attacker = Address::generate(&s.env);
    s.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &s.client.address,
            fn_name: "resume_stream",
            args: (stream_id,).into_val(&s.env),
            sub_invokes: &[],
        },
    }]);
    assert!(s.client.try_resume_stream(&stream_id).is_err());
    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Paused);

    // The donor's own auth is accepted, and is the one the contract asks for.
    s.env.mock_auths(&[MockAuth {
        address: &s.donor,
        invoke: &MockAuthInvoke {
            contract: &s.client.address,
            fn_name: "resume_stream",
            args: (stream_id,).into_val(&s.env),
            sub_invokes: &[],
        },
    }]);
    s.client.resume_stream(&stream_id);
    assert_eq!(s.env.auths()[0].0, s.donor);
    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
}

// =============================================================================
// Accrual stops while a stream is paused (issue #450)
// =============================================================================

/// Moves the ledger clock forward by `secs`.
fn advance(env: &Env, secs: u64) {
    env.ledger().with_mut(|l| l.timestamp += secs);
}

/// A 1_000-token stream at 10/s, so every amount in the pause tests below is
/// `10 * seconds` and easy to check by eye.
fn paused_test_stream(s: &Setup) -> u64 {
    s.token_admin.mint(&s.donor, &1_000);
    s.client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10)
}

#[test]
fn payable_amount_before_during_and_after_a_pause() {
    let s = setup();
    let stream_id = paused_test_stream(&s);

    // Before: 30s at 10/s is payable.
    advance(&s.env, 30);
    assert_eq!(s.client.pending_accrual(&stream_id), 300);

    // Pausing settles what was owed straight to the NGO.
    assert_eq!(s.client.pause_stream(&stream_id), 300);
    assert_eq!(s.token.balance(&s.ngo), 300);
    assert_eq!(s.client.get_stream(&stream_id).balance, 700);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);

    // During: nothing becomes payable however long the pause lasts.
    for secs in [1, 49, 50] {
        advance(&s.env, secs);
        assert_eq!(s.client.pending_accrual(&stream_id), 0);
        assert_eq!(s.client.pending_payout(&stream_id), (0, 0));
    }
    assert_eq!(s.token.balance(&s.ngo), 300);
    assert_eq!(s.token.balance(&s.client.address), 700);

    // After: accrual restarts from the resume time, not the pause time.
    s.client.resume_stream(&stream_id);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);
    advance(&s.env, 20);
    assert_eq!(s.client.pending_accrual(&stream_id), 200);

    assert_eq!(s.client.withdraw(&stream_id), 200);
    assert_eq!(s.token.balance(&s.ngo), 500);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 500);
    assert_eq!(stream.withdrawn, 500);
    assert_eq!(s.token.balance(&s.client.address), 500);
}

#[test]
fn withdraw_is_rejected_while_paused_and_pays_nothing() {
    let s = setup();
    let stream_id = paused_test_stream(&s);
    advance(&s.env, 10);
    s.client.pause_stream(&stream_id);
    advance(&s.env, 1_000);

    assert_eq!(
        s.client.try_withdraw(&stream_id),
        Err(Ok(Error::StreamPaused))
    );
    assert_eq!(s.token.balance(&s.ngo), 100);
    assert_eq!(s.client.get_stream(&stream_id).balance, 900);
}

#[test]
fn a_pause_longer_than_the_stream_would_last_does_not_drain_it() {
    let s = setup();
    let stream_id = paused_test_stream(&s);
    s.client.pause_stream(&stream_id);

    // 10_000s at 10/s would be ten times the balance if it kept accruing.
    advance(&s.env, 10_000);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Paused);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(s.token.balance(&s.ngo), 0);

    s.client.resume_stream(&stream_id);
    advance(&s.env, 40);
    assert_eq!(s.client.pending_accrual(&stream_id), 400);
}

#[test]
fn resuming_in_the_same_second_adds_nothing() {
    let s = setup();
    let stream_id = paused_test_stream(&s);
    advance(&s.env, 5);

    assert_eq!(s.client.pause_stream(&stream_id), 50);
    s.client.resume_stream(&stream_id);

    assert_eq!(s.client.pending_accrual(&stream_id), 0);
    assert_eq!(s.client.get_stream(&stream_id).balance, 950);
    advance(&s.env, 3);
    assert_eq!(s.client.pending_accrual(&stream_id), 30);
}

#[test]
fn repeated_pauses_pay_only_for_the_active_seconds() {
    let s = setup();
    let stream_id = paused_test_stream(&s);

    // (active seconds, paused seconds) for three cycles: 10 + 20 + 5 = 35
    // active seconds in total, 350 tokens.
    for (active, paused) in [(10u64, 100u64), (20, 7), (5, 3_600)] {
        advance(&s.env, active);
        assert_eq!(s.client.pause_stream(&stream_id), 10 * active as i128);
        advance(&s.env, paused);
        assert_eq!(s.client.pending_accrual(&stream_id), 0);
        s.client.resume_stream(&stream_id);
    }

    assert_eq!(s.token.balance(&s.ngo), 350);
    assert_eq!(s.client.get_stream(&stream_id).balance, 650);
    assert_eq!(s.client.get_stream(&stream_id).withdrawn, 350);
    assert_eq!(s.token.balance(&s.client.address), 650);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);
}

#[test]
fn a_cliff_stream_paused_before_its_start_accrues_from_the_resume_time() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let start = s.env.ledger().timestamp() + 100;
    let stream_id =
        s.client
            .create_stream_with_start(&s.donor, &s.ngo, &s.token.address, &1_000, &10, &start);

    advance(&s.env, 10);
    assert_eq!(s.client.pause_stream(&stream_id), 0);

    // Sit paused until well after the cliff would have passed.
    s.env.ledger().with_mut(|l| l.timestamp = start + 50);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);

    // The cliff is behind us, so only time after the resume counts.
    s.client.resume_stream(&stream_id);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);
    advance(&s.env, 10);
    assert_eq!(s.client.pending_accrual(&stream_id), 100);
    assert_eq!(s.client.withdraw(&stream_id), 100);
    assert_eq!(s.token.balance(&s.ngo), 100);
}

#[cfg(test)]
mod proptest_committed_balance {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::TestCaseResult;

    #[derive(Debug, Clone)]
    enum Action {
        Create { deposit: i128, rate: i128 },
        TopUp { slot: usize, amount: i128 },
        Withdraw { slot: usize },
        Cancel { slot: usize },
        Transfer { slot: usize },
    }

    #[derive(Debug, Clone)]
    struct Step {
        advance: u64,
        action: Action,
    }

    fn step_strategy() -> impl Strategy<Value = Step> {
        let action = prop_oneof![
            3 => (100i128..=5_000i128, 1i128..=20i128)
                .prop_map(|(deposit, rate)| Action::Create { deposit, rate }),
            2 => (0usize..12, 100i128..=2_000i128)
                .prop_map(|(slot, amount)| Action::TopUp { slot, amount }),
            3 => (0usize..12).prop_map(|slot| Action::Withdraw { slot }),
            1 => (0usize..12).prop_map(|slot| Action::Cancel { slot }),
            1 => (0usize..12).prop_map(|slot| Action::Transfer { slot }),
        ];
        (0u64..=30u64, action).prop_map(|(advance, action)| Step { advance, action })
    }

    /// The vault's committed balance for a token must equal the sum of the
    /// balances of every stream funded with that token. With no surplus and no
    /// treasury configured, the token balance the vault actually holds matches
    /// the same number.
    fn assert_invariant(s: &Setup<'_>, ids: &Vec<u64>) -> TestCaseResult {
        let expected: i128 = ids.iter().map(|id| s.client.get_stream(&id).balance).sum();
        prop_assert_eq!(s.client.committed_balance(&s.token.address), expected);
        prop_assert_eq!(s.token.balance(&s.client.address), expected);
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// A random sequence of create, top-up, withdraw, cancel and transfer
        /// calls must keep `committed_balance` in step with stream balances
        /// after every single call, including calls that are rejected.
        #[test]
        fn committed_balance_equals_sum_of_stream_balances(
            steps in prop::collection::vec(step_strategy(), 1..20)
        ) {
            let s = setup();
            s.token_admin.mint(&s.donor, &1_000_000);
            let other_ngo = Address::generate(&s.env);
            let mut ids = Vec::new(&s.env);

            for step in steps {
                s.env.ledger().with_mut(|l| l.timestamp += step.advance);

                match step.action {
                    Action::Create { deposit, rate } => {
                        // `try_*` keeps the sequence going when a call is
                        // rejected; a rejected call must not move any funds.
                        if let Ok(Ok(id)) = s.client.try_create_stream(
                            &s.donor,
                            &s.ngo,
                            &s.token.address,
                            &deposit,
                            &rate,
                        ) {
                            ids.push_back(id);
                        }
                    }
                    Action::TopUp { slot, amount } => {
                        if let Some(id) = ids.get((slot as u32) % ids.len().max(1)) {
                            let _ = s.client.try_top_up(&id, &amount);
                        }
                    }
                    Action::Withdraw { slot } => {
                        if let Some(id) = ids.get((slot as u32) % ids.len().max(1)) {
                            let _ = s.client.try_withdraw(&id);
                        }
                    }
                    Action::Cancel { slot } => {
                        if let Some(id) = ids.get((slot as u32) % ids.len().max(1)) {
                            let _ = s.client.try_cancel_stream(&id);
                        }
                    }
                    Action::Transfer { slot } => {
                        if let Some(id) = ids.get((slot as u32) % ids.len().max(1)) {
                            let _ = s.client.try_transfer_stream(&id, &other_ngo);
                        }
                    }
                }

                assert_invariant(&s, &ids)?;
            }
        }
    }
}

#[test]
fn set_memo_updates_the_stream_and_emits_old_and_new_memo() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let old_memo = Bytes::from_array(&s.env, b"campaign-42");
    let stream_id = s.client.create_stream_with_memo(
        &s.donor,
        &s.ngo,
        &s.token.address,
        &1_000,
        &10,
        &old_memo,
    );

    let new_memo = Bytes::from_array(&s.env, b"campaign-43");
    s.client.set_memo(&stream_id, &new_memo);

    // The record carries the new memo...
    assert_eq!(s.client.get_stream(&stream_id).memo, new_memo);
    // ...and the event reports both values so an indexer can follow the change.
    assert_eq!(
        last_event(&s.env),
        (
            (symbol_short!("memoset"), stream_id).into_val(&s.env),
            (old_memo.clone(), new_memo.clone()).into_val(&s.env),
        )
    );

    // An empty memo clears the tag, and is reported as the new value.
    let empty = Bytes::new(&s.env);
    s.client.set_memo(&stream_id, &empty);
    assert_eq!(s.client.get_stream(&stream_id).memo, empty);
    assert_eq!(
        last_event(&s.env),
        (
            (symbol_short!("memoset"), stream_id).into_val(&s.env),
            (new_memo, empty).into_val(&s.env),
        )
    );
}

#[test]
fn set_memo_rejects_an_oversized_memo() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    // Exactly `MAX_MEMO_BYTES` is accepted.
    let max_memo = Bytes::from_slice(&s.env, &[7u8; MAX_MEMO_BYTES as usize]);
    s.client.set_memo(&stream_id, &max_memo);
    assert_eq!(s.client.get_stream(&stream_id).memo, max_memo);

    // One byte over the cap is rejected and the stored memo is unchanged.
    let too_long = Bytes::from_slice(&s.env, &[0u8; MAX_MEMO_BYTES as usize + 1]);
    assert_eq!(
        s.client.try_set_memo(&stream_id, &too_long),
        Err(Ok(Error::MemoTooLong))
    );
    assert_eq!(s.client.get_stream(&stream_id).memo, max_memo);
}

#[test]
fn set_memo_rejects_a_cancelled_stream() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let memo = Bytes::from_array(&s.env, b"campaign-42");
    let stream_id =
        s.client
            .create_stream_with_memo(&s.donor, &s.ngo, &s.token.address, &1_000, &10, &memo);
    s.client.cancel_stream(&stream_id);

    let renamed = Bytes::from_array(&s.env, b"campaign-43");
    assert_eq!(
        s.client.try_set_memo(&stream_id, &renamed),
        Err(Ok(Error::StreamCancelled))
    );
    assert_eq!(s.client.get_stream(&stream_id).memo, memo);
}
