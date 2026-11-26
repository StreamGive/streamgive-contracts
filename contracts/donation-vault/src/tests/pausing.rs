// SPDX-License-Identifier: Apache-2.0
//! Tests for vault pause/unpause, stream-level pause/resume,
//! and admin rescue of streams during a vault pause.

use soroban_sdk::testutils::{Ledger, MockAuth, MockAuthInvoke};
use soroban_sdk::{symbol_short, IntoVal, Vec};

use crate::tests::helpers::*;
use crate::*;

// ── Vault pause / unpause ─────────────────────────────────────────────────────

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
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    s.client.pause();

    let result = s.client.try_withdraw(&stream_id);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));

    assert_eq!(s.token.balance(&s.ngo), 0);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);

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

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 10);

    s.client.pause();
    assert!(s.client.paused());

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

    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 900);
    assert_eq!(s.token.balance(&s.ngo), 100);
    assert_eq!(s.token.balance(&s.donor), 2_100);

    s.token_admin.mint(&s.donor, &500);
    let stream_id2 = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &500, &10);
    s.client.pause();
    let rescued = s.client.rescue_stream(&stream_id2);
    assert_eq!(rescued, 500);
    assert_eq!(s.token.balance(&s.donor), 2_600);

    assert!(s.client.get_stream(&stream_id).cancelled);
    assert_eq!(s.client.pending_accrual(&stream_id2), 0);
    assert_eq!(s.client.depletion_time(&stream_id2), None);
    assert_eq!(s.client.total_donated(), 1_500);

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

    s.client.unpause();
    assert!(!s.client.paused());
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

// ── Rescue stream ─────────────────────────────────────────────────────────────

#[test]
fn rescue_stream_refunds_donor_and_updates_state_and_emits_event() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 20);

    assert_eq!(s.token.balance(&s.donor), 0);
    assert_eq!(s.token.balance(&s.client.address), 1_000);
    assert_eq!(s.token.balance(&s.ngo), 0);

    s.client.pause();

    let refund = s.client.rescue_stream(&stream_id);
    assert_eq!(refund, 1_000);
    let evt = last_event(&s.env);

    assert_eq!(s.token.balance(&s.donor), 1_000);
    assert_eq!(s.token.balance(&s.client.address), 0);
    assert_eq!(s.token.balance(&s.ngo), 0);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.rate, 0);
    assert!(stream.cancelled);
    assert_eq!(stream.status, StreamStatus::Cancelled);
    assert_eq!(stream.last_update, s.env.ledger().timestamp());
    assert_eq!(stream.withdrawn, 0);

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

    s.client.cancel_stream(&stream_id);

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

// ── Stream-level pause / resume ───────────────────────────────────────────────

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
    assert_eq!(s.client.withdraw(&stream_id), 100);
    assert_eq!(s.client.total_withdrawn(), 200);

    s.client.cancel_stream(&stream_id);
    let config = s.client.get_config();
    assert_eq!(config.total_withdrawn, 200);
    assert_eq!(config.total_fees_collected, 100);
}
