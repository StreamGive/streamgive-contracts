// SPDX-License-Identifier: Apache-2.0
//! Tests for stream lifecycle: create, withdraw, cancel, top-up, modify_rate,
//! transfer, depletion, TTL, status, streams_by_donor/ngo, and arithmetic
//! edge cases.

use soroban_sdk::testutils::{Ledger, MockAuth, MockAuthInvoke};
use soroban_sdk::{symbol_short, Bytes, IntoVal, Vec};

use crate::tests::helpers::*;
use crate::*;

// ── Create / basic lifecycle ──────────────────────────────────────────────────

#[test]
fn full_lifecycle_create_accrue_withdraw_cancel() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

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

    s.env.ledger().with_mut(|l| l.timestamp += 20);
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
fn concurrent_streams_to_one_ngo_accrue_and_pay_out_independently() {
    let s = setup();
    let donor_b = Address::generate(&s.env);
    s.token_admin.mint(&s.donor, &1_000);
    s.token_admin.mint(&donor_b, &600);

    let id_a = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 20);
    let id_b = s
        .client
        .create_stream(&donor_b, &s.ngo, &s.token.address, &600, &5);
    assert_ne!(id_a, id_b);
    assert_eq!(s.token.balance(&s.client.address), 1_600);

    s.env.ledger().with_mut(|l| l.timestamp += 30);
    assert_eq!(s.client.pending_accrual(&id_a), 500);
    assert_eq!(s.client.pending_accrual(&id_b), 150);

    assert_eq!(s.client.withdraw(&id_a), 500);
    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.client.pending_accrual(&id_b), 150);

    assert_eq!(s.client.withdraw(&id_b), 150);
    assert_eq!(s.token.balance(&s.ngo), 650);

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

    s.env.ledger().with_mut(|l| l.timestamp += 10);

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
    assert_eq!(stream.balance, 1_400);
    assert_eq!(stream.rate, 10);

    s.env.ledger().with_mut(|l| l.timestamp += 5);

    s.client.modify_rate(&stream_id, &20);
    assert_last_event(
        &s.env,
        (symbol_short!("ratemod"), stream_id),
        (10i128, 20i128, 50i128),
    );
    assert_eq!(s.token.balance(&s.ngo), 150);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.rate, 20);
    assert_eq!(stream.balance, 1_350);
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

    s.env.ledger().with_mut(|l| l.timestamp += 50);
    s.client.withdraw(&stream_id);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.created_at, created_at);
    assert_ne!(stream.last_update, created_at);
}

#[test]
fn create_stream_stores_every_field() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.env.ledger().with_mut(|l| l.timestamp = 12_345);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

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

    s.env.as_contract(&s.client.address, || {
        s.env.storage().instance().remove(&DataKey::NextStreamId);
    });

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::StreamCounterMissing)));

    assert_eq!(
        s.client.try_get_stream(&0u64),
        Err(Ok(Error::StreamNotFound))
    );
}

#[test]
fn create_stream_rejects_self_stream() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);

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

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &999, &10);
    assert_eq!(result, Err(Ok(Error::DepositTooLow)));

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

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

    assert_eq!(s.client.stream_count(), 0);
    assert_eq!(s.client.total_donated(), 0);
    assert_eq!(s.client.streams_by_donor(&s.donor).len(), 0);
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

    assert_eq!(s.client.stream_count(), 0);
    assert_eq!(s.client.total_donated(), 0);
    assert_eq!(s.client.streams_by_donor(&s.donor).len(), 0);
    assert_eq!(s.token.balance(&s.donor), donor_balance_before);
}

#[test]
fn create_stream_checks_vault_address_before_other_validations() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let vault = s.client.address.clone();

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

    s.env.ledger().with_mut(|l| l.timestamp += 10);
    s.client.top_up(&stream_id, &500);
    assert_eq!(s.token.balance(&s.ngo), 100);

    let result = s.client.try_withdraw(&stream_id);
    assert_eq!(result, Err(Ok(Error::NothingToWithdraw)));
    assert_eq!(s.token.balance(&s.ngo), 100);
}

#[test]
fn withdraw_and_cancel_on_fully_drained_stream_are_no_ops() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 100);

    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 1_000);
    assert_eq!(s.token.balance(&s.ngo), 1_000);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.withdrawn, 1_000);
    assert!(!stream.cancelled);
    assert_eq!(stream.rate, 10);

    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let result = s.client.try_withdraw(&stream_id);
    assert_eq!(result, Err(Ok(Error::NothingToWithdraw)));

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

    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 500);
    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.token.balance(&s.donor), 500);

    let stream = s.client.get_stream(&stream_id);
    let last_update_before_second_cancel = stream.last_update;

    s.env.ledger().with_mut(|l| l.timestamp += 50);
    let result = s.client.try_cancel_stream(&stream_id);
    assert_eq!(result, Err(Ok(Error::StreamCancelled)));

    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.token.balance(&s.donor), 500);

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

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);
    assert_eq!(s.token.balance(&s.donor), 500);
    assert_eq!(s.token.balance(&s.ngo), 0);
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

    let result = s.client.try_top_up(&stream_id, &50);
    assert_eq!(result, Err(Ok(Error::DepositTooLow)));

    s.client.top_up(&stream_id, &100);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_100);
}

#[test]
fn get_stream_on_unknown_id_fails() {
    let s = setup();
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

// ── Pending accrual / payout / refund views ───────────────────────────────────

#[test]
fn pending_accrual_matches_withdraw_without_mutating_state() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let pending = s.client.pending_accrual(&stream_id);
    assert_eq!(pending, 500);

    assert_eq!(s.token.balance(&s.ngo), 0);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);

    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, pending);
}

#[test]
fn pending_refund_of_an_active_stream_matches_the_cancel_refund() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 20);

    assert_eq!(s.client.pending_refund(&stream_id), 800);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);
    assert_eq!(s.token.balance(&s.ngo), 0);

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
    s.env.ledger().with_mut(|l| l.timestamp += 500);

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
    s.env.ledger().with_mut(|l| l.timestamp += 50);

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

    assert_eq!(s.client.pending_refund(&stream_id), 1_000);

    s.env.ledger().with_mut(|l| l.timestamp = start + 20);
    assert_eq!(s.client.pending_refund(&stream_id), 800);
}

#[test]
fn pending_refund_on_unknown_stream_fails() {
    let s = setup();
    let result = s.client.try_pending_refund(&999);
    assert_eq!(result, Err(Ok(Error::StreamNotFound)));
}

// ── Depletion time ────────────────────────────────────────────────────────────

#[test]
fn depletion_time_is_last_update_plus_balance_over_rate() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    let created_at = s.client.get_stream(&stream_id).created_at;

    assert_eq!(s.client.depletion_time(&stream_id), Some(created_at + 100));

    s.env.ledger().with_mut(|l| l.timestamp += 30);
    assert_eq!(s.client.depletion_time(&stream_id), Some(created_at + 100));
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_000);
}

#[test]
fn depletion_time_rounds_a_partial_final_second_up() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &300);
    let created_at = s.client.get_stream(&stream_id).created_at;
    let depletes_at = s.client.depletion_time(&stream_id).unwrap();
    assert_eq!(depletes_at, created_at + 4);

    s.env.ledger().with_mut(|l| l.timestamp = depletes_at - 1);
    assert_eq!(s.client.pending_accrual(&stream_id), 900);

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

    s.env.ledger().with_mut(|l| l.timestamp += 2);
    s.client.withdraw(&stream_id);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 400);

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

// ── TTL / storage bumps ───────────────────────────────────────────────────────

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

    s.client.cancel_stream(&stream_id);
    age_past_thresholds(&s, Some(stream_id));

    s.client.extend_stream(&stream_id);

    assert_eq!(stream_ttl(&s, stream_id), STREAM_BUMP_AMOUNT);
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

// ── Stream status state machine ───────────────────────────────────────────────

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
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    let result = s.client.try_withdraw(&stream_id);
    assert_eq!(result, Err(Ok(Error::NothingToWithdraw)));
    s.env.ledger().with_mut(|l| l.timestamp += 10);
    let result = s.client.try_withdraw(&stream_id);
    assert!(result.is_err());
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

    assert_eq!(s.client.cancel_stream(&stream_id), 0);
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Cancelled
    );

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

#[test]
fn modify_rate_and_top_up_on_a_drained_stream() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 100);
    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 1_000);

    let drained = s.client.get_stream(&stream_id);
    assert_eq!(drained.balance, 0);
    assert_eq!(drained.status, StreamStatus::Drained);
    assert!(!drained.cancelled);

    let modify = s.client.try_modify_rate(&stream_id, &20);
    assert_eq!(modify, Ok(Ok(())));
    let after_modify = s.client.get_stream(&stream_id);
    assert_eq!(after_modify.rate, 20);
    assert_eq!(after_modify.balance, 0);
    assert_eq!(after_modify.status, StreamStatus::Drained);

    let top_up = s.client.try_top_up(&stream_id, &500);
    assert_eq!(top_up, Ok(Ok(())));
    let after_top_up = s.client.get_stream(&stream_id);
    assert_eq!(after_top_up.balance, 500);
    assert_eq!(after_top_up.rate, 20);
    assert_eq!(after_top_up.status, StreamStatus::Active);

    s.env.ledger().with_mut(|l| l.timestamp += 10);
    assert_eq!(s.client.pending_accrual(&stream_id), 200);
}

#[test]
fn settle_marks_stream_drained_when_balance_reaches_zero() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 100);
    s.client.modify_rate(&stream_id, &20);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.rate, 20);
    assert_eq!(stream.status, StreamStatus::Drained);
    assert!(!stream.cancelled);

    assert_eq!(s.token.balance(&s.ngo), 1_000);

    s.client.modify_rate(&stream_id, &5);
    let after = s.client.get_stream(&stream_id);
    assert_eq!(after.rate, 5);
    assert_eq!(after.status, StreamStatus::Drained);
}

// ── Drained event ─────────────────────────────────────────────────────────────

#[test]
fn withdraw_that_empties_stream_emits_drained_event() {
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
    assert_nth_last_event(
        &s.env,
        1,
        (symbol_short!("withdraw"), stream_id),
        (1_000i128, 0i128, 1_000i128, s.ngo.clone()),
    );
    assert_last_event(&s.env, (symbol_short!("drained"), stream_id), (1_000i128,));
}

#[test]
fn withdraw_that_leaves_balance_emits_no_drained_event() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    s.client.withdraw(&stream_id);

    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
    assert_last_event(
        &s.env,
        (symbol_short!("withdraw"), stream_id),
        (500i128, 0i128, 500i128, s.ngo.clone()),
    );
    assert_eq!(count_events_with_topic(&s.env, symbol_short!("drained")), 0);
}

#[test]
fn later_withdraw_that_empties_stream_reports_cumulative_total_paid() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);
    s.client.withdraw(&stream_id);

    s.env.ledger().with_mut(|l| l.timestamp += 50);
    s.client.withdraw(&stream_id);

    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );
    assert_last_event(&s.env, (symbol_short!("drained"), stream_id), (1_000i128,));
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

    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );
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

    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );
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
    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Drained
    );
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
}

#[test]
fn cancel_stream_that_settles_to_zero_emits_no_drained_event() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 100);

    s.client.cancel_stream(&stream_id);

    assert_eq!(
        s.client.get_stream(&stream_id).status,
        StreamStatus::Cancelled
    );
    assert_eq!(count_events_with_topic(&s.env, symbol_short!("drained")), 0);
    assert_last_event(
        &s.env,
        (symbol_short!("cancel"), stream_id),
        (1_000i128, 0i128),
    );
}

#[test]
fn top_up_that_settles_to_zero_emits_no_drained_event() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 100);

    s.client.top_up(&stream_id, &1_000);

    assert_eq!(s.client.get_stream(&stream_id).status, StreamStatus::Active);
    assert_eq!(count_events_with_topic(&s.env, symbol_short!("drained")), 0);
}

// ── Transfer stream ───────────────────────────────────────────────────────────

#[test]
fn transfer_stream_settles_accrual_to_old_ngo_and_points_at_new_ngo() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let new_ngo = Address::generate(&s.env);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 50);

    s.client.transfer_stream(&stream_id, &new_ngo);

    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.token.balance(&new_ngo), 0);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.ngo, new_ngo);
    assert_eq!(stream.balance, 500);
    assert_eq!(stream.withdrawn, 500);
    assert_eq!(stream.rate, 10);
    assert!(!stream.cancelled);

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
fn transfer_stream_emits_event_with_settled_accrual() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    let new_ngo = Address::generate(&s.env);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

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
    let result = s.client.try_transfer_stream(&stream_id, &new_ngo);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    registry.register(&new_ngo, &soroban_sdk::String::from_str(&s.env, "New NGO"));
    registry.approve_ngo(&s.client.admin(), &new_ngo);
    assert_eq!(
        s.client.try_transfer_stream(&stream_id, &new_ngo),
        Ok(Ok(()))
    );
    assert_eq!(s.client.get_stream(&stream_id).ngo, new_ngo);
}

// ── streams_by_donor ──────────────────────────────────────────────────────────

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

    assert_eq!(id_a, 0);
    assert_eq!(id_b, id_a + 1);
    assert_eq!(id_c, id_b + 1);
    assert_eq!(s.client.stream_count(), 3);
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

// ── Cliff / create_stream_with_start ─────────────────────────────────────────

#[test]
fn cliff_stream_accrues_nothing_before_start() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);

    let start = 1_500;
    let stream_id =
        s.client
            .create_stream_with_start(&s.donor, &s.ngo, &s.token.address, &1_000, &10, &start);

    assert_eq!(s.client.get_stream(&stream_id).start, start);
    assert_eq!(s.token.balance(&s.donor), 0);

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

    s.env.ledger().with_mut(|l| l.timestamp = 1_400);
    assert_eq!(s.client.pending_accrual(&stream_id), 0);

    s.env.ledger().with_mut(|l| l.timestamp = start + 50);
    assert_eq!(s.client.pending_accrual(&stream_id), 500);

    assert_eq!(s.client.withdraw(&stream_id), 500);
    assert_eq!(s.client.get_stream(&stream_id).withdrawn, 500);
}

#[test]
fn cliff_depletion_time_is_measured_from_start() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);
    s.env.ledger().with_mut(|l| l.timestamp = 1_000);

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

// ── Memo ──────────────────────────────────────────────────────────────────────

#[test]
fn create_stream_with_memo_stores_and_emits_the_memo() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let memo = Bytes::from_array(&s.env, b"campaign-42");
    let stream_id =
        s.client
            .create_stream_with_memo(&s.donor, &s.ngo, &s.token.address, &1_000, &10, &memo);

    let created = last_event(&s.env);

    assert_eq!(s.client.get_stream(&stream_id).memo, memo);
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

// ── Per-donor stream limits ───────────────────────────────────────────────────

#[test]
fn create_stream_rejects_the_101st_stream() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000_000);

    for _ in 0..100 {
        s.client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    }

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::StreamLimitExceeded)));

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

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::StreamLimitExceeded)));

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

    s.client.set_max_streams_per_donor(&1);

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::StreamLimitExceeded)));

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
    let donor_b_stream = s
        .client
        .create_stream(&donor_b, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_stream(&donor_b_stream).donor, donor_b);

    let result = s
        .client
        .try_create_stream(&donor_b, &s.ngo, &s.token.address, &1_000, &10);
    assert!(result.is_err());
}

#[test]
fn default_stream_cap_allows_exactly_100_streams() {
    let s = setup();
    let balance_per_stream: i128 = 1_000;
    let total_balance = balance_per_stream * 100;
    s.token_admin.mint(&s.donor, &total_balance);

    for _ in 0..100 {
        s.client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &balance_per_stream, &10);
    }

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

    for _ in 0..100 {
        s.client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &balance_per_stream, &10);
    }

    let streams = s.client.streams_by_donor(&s.donor);
    assert_eq!(streams.len(), 100);
}

// ── max_rate and min_duration ─────────────────────────────────────────────────

#[test]
fn create_stream_enforces_max_rate_boundary() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);
    s.client.set_max_rate(&100);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &100);
    assert_eq!(s.client.get_stream(&stream_id).rate, 100);

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

    s.client.modify_rate(&stream_id, &100);
    assert_eq!(s.client.get_stream(&stream_id).rate, 100);

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

    let first = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &1_000_000);
    assert_eq!(s.client.get_stream(&first).rate, 1_000_000);

    s.client.set_max_rate(&100);
    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &1_000_000),
        Err(Ok(Error::RateTooHigh))
    );

    s.client.clear_max_rate();
    let second = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &1_000_000);
    assert_eq!(s.client.get_stream(&second).rate, 1_000_000);
}

#[test]
fn create_stream_enforces_min_duration_boundary() {
    let s = setup();
    s.token_admin.mint(&s.donor, &10_000);
    s.client.set_min_duration(&60);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &600, &10);
    assert_eq!(s.client.get_stream(&stream_id).rate, 10);

    let second = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &591, &10);
    assert_eq!(s.client.get_stream(&second).balance, 591);

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

    assert_eq!(
        s.client
            .try_create_stream(&s.donor, &s.ngo, &s.token.address, &10, &10),
        Err(Ok(Error::StreamTooShort))
    );

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

    s.client.modify_rate(&stream_id, &16);
    assert_eq!(s.client.get_stream(&stream_id).rate, 16);

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

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1, &1);
    assert_eq!(s.client.get_stream(&stream_id).rate, 1);
}

// ── Arithmetic overflow guards ────────────────────────────────────────────────

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

#[test]
fn accrual_after_very_long_elapsed_time_is_capped_by_balance() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000_000, &1_000);

    s.env.ledger().with_mut(|l| l.timestamp += u64::MAX - 1000);

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

    s.env.ledger().with_mut(|l| l.timestamp += u64::MAX);

    let accrued = s.client.pending_accrual(&stream_id);
    assert_eq!(accrued, balance);
}

// ── Auth guards ───────────────────────────────────────────────────────────────

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
#[should_panic]
fn withdraw_fails_for_non_ngo_caller() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 50);

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

// ── Storage layout ────────────────────────────────────────────────────────────

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
    use soroban_sdk::xdr::ScVal;
    use soroban_sdk::{IntoVal, TryFromVal, Val};

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
                    soroban_sdk::Symbol::try_from_val(&s.env, &entry.key)
                        .expect("struct fields encode as symbols"),
                );
            }
            keys
        }
        other => panic!("expected a struct map, got {other:?}"),
    };

    let mut expected = Vec::new(&s.env);
    for field in STREAM_FIELDS {
        expected.push_back(soroban_sdk::Symbol::new(&s.env, field));
    }
    assert_eq!(
        keys, expected,
        "Stream's field list changed; update STREAM_FIELDS"
    );
}

#[test]
fn previous_stream_layout_is_not_decodable_by_current_code() {
    use soroban_sdk::{symbol_short, IntoVal, Map, Symbol, TryFromVal, Val};

    let s = setup();

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

// ── total_donated ─────────────────────────────────────────────────────────────

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

// ── Roles doc self-test ───────────────────────────────────────────────────────

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

#[test]
fn roles_doc_lists_every_entry_point() {
    const SOURCE: &str = include_str!("../lib.rs");
    const ROLES: &str = include_str!("../../../../docs/ROLES.md");
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
