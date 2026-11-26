// SPDX-License-Identifier: Apache-2.0
//! Tests for `withdraw_batch`, `withdraw_all` pagination, and related batch
//! error conditions.

use soroban_sdk::testutils::{Ledger, MockAuth, MockAuthInvoke};
use soroban_sdk::{symbol_short, IntoVal, Vec};

use crate::tests::helpers::*;
use crate::*;

// ── withdraw_batch ────────────────────────────────────────────────────────────

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
fn withdraw_batch_across_several_streams() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);

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

    assert_eq!(amounts.get(0), Some(475));
    assert_eq!(amounts.get(1), Some(950));
    assert_eq!(amounts.get(2), Some(475));

    let gross: i128 = 500 + 1_000 + 500;
    let fee: i128 = (gross * 500) / 10_000;
    let net: i128 = gross - fee;
    assert_eq!(gross, net + fee);

    assert_eq!(s.token.balance(&s.ngo), net);
    assert_eq!(s.token.balance(&treasury), fee);
    assert_eq!(s.token.balance(&s.client.address), 4_000 - gross);

    let stream1 = s.client.get_stream(&s1);
    assert_eq!(stream1.balance, 500);
    assert_eq!(stream1.withdrawn, 500);
    assert_eq!(stream1.status, StreamStatus::Active);
    assert_eq!(stream1.last_update, s.env.ledger().timestamp());

    let stream2 = s.client.get_stream(&s2);
    assert_eq!(stream2.balance, 1_000);
    assert_eq!(stream2.withdrawn, 1_000);
    assert_eq!(stream2.status, StreamStatus::Active);

    let stream3 = s.client.get_stream(&s3);
    assert_eq!(stream3.balance, 500);
    assert_eq!(stream3.withdrawn, 500);
    assert_eq!(stream3.status, StreamStatus::Active);
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

    s.client.withdraw(&s1);

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
    s.client.set_fee_bps(&500);

    let token_b_admin_addr = Address::generate(&s.env);
    let (token_b, token_b_admin) = create_token(&s.env, &token_b_admin_addr);
    s.client.set_token_fee_bps(&token_b.address, &1_000);

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

    let amounts = s.client.withdraw_batch(&ids);
    assert_eq!(amounts.get(0), Some(475));
    assert_eq!(amounts.get(1), Some(475));
    assert_eq!(amounts.get(2), Some(900));

    let token_a_gross: i128 = 1_000;
    let token_a_fee: i128 = 50;
    let token_a_net: i128 = 950;
    assert_eq!(token_a_gross, token_a_net + token_a_fee);
    assert_eq!(s.token.balance(&s.ngo), token_a_net);
    assert_eq!(s.token.balance(&treasury), token_a_fee);

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
fn withdraw_batch_emits_drained_event_after_the_streams_withdraw() {
    let s = setup();
    s.token_admin.mint(&s.donor, &3_000);

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

    assert_eq!(s.client.get_stream(&s1).status, StreamStatus::Drained);
    assert_eq!(s.client.get_stream(&s2).status, StreamStatus::Active);

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

    assert_eq!(s.client.get_stream(&s1).status, StreamStatus::Drained);
    assert_eq!(s.client.get_stream(&s2).status, StreamStatus::Drained);

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
}

// ── withdraw_all ──────────────────────────────────────────────────────────────

#[test]
fn withdraw_all_pages_through_more_streams_than_the_page_size() {
    let s = setup();
    let count: u32 = 25;
    s.token_admin.mint(&s.donor, &(count as i128 * 1_000));
    for _ in 0..count {
        s.client
            .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    }

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

    assert_eq!(calls, 3);
    assert_eq!(total_paid, count as i128 * 500);
    assert_eq!(s.token.balance(&s.ngo), count as i128 * 500);

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
