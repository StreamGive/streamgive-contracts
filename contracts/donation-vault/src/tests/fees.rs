// SPDX-License-Identifier: Apache-2.0
//! Tests for fee configuration, treasury, per-token fee overrides,
//! NGO payout address, and surplus sweep.

use soroban_sdk::testutils::Ledger;
use soroban_sdk::{symbol_short, IntoVal};

use crate::tests::helpers::*;
use crate::*;

// ── Global fee / treasury ─────────────────────────────────────────────────────

#[test]
fn withdraw_with_no_treasury_takes_no_fee() {
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
    s.client.set_fee_bps(&500);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.env.ledger().with_mut(|l| l.timestamp += 20);
    s.client.withdraw(&stream_id);
    assert_eq!(s.token.balance(&treasury), 10);
    assert_eq!(s.token.balance(&s.ngo), 190);

    s.client.set_fee_bps(&0);
    s.client.clear_treasury();
    assert_eq!(s.client.treasury(), None);

    s.env.ledger().with_mut(|l| l.timestamp += 20);
    s.client.withdraw(&stream_id);
    assert_eq!(s.token.balance(&s.ngo), 390);
    assert_eq!(s.token.balance(&treasury), 10);
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
    s.client.set_fee_bps(&500);
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
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 475);
    assert_eq!(s.token.balance(&treasury), 25);
    assert_eq!(s.token.balance(&s.ngo), 475);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.withdrawn, 500);
}

#[test]
fn withdraw_returns_net_after_protocol_fee() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&250);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 40);

    let ngo_before = s.token.balance(&s.ngo);
    let withdrawn = s.client.withdraw(&stream_id);

    assert_eq!(withdrawn, 390);
    assert_eq!(s.token.balance(&s.ngo) - ngo_before, withdrawn);
    assert_eq!(s.token.balance(&treasury), 10);
    assert_eq!(s.client.get_stream(&stream_id).withdrawn, 400);
}

#[test]
fn cancel_stream_splits_protocol_fee_on_accrued_but_not_on_refund() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 20);

    let refund = s.client.cancel_stream(&stream_id);
    assert_eq!(refund, 800);
    assert_eq!(s.token.balance(&treasury), 10);
    assert_eq!(s.token.balance(&s.ngo), 190);
    assert_eq!(s.token.balance(&s.donor), 800);

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 0);
    assert_eq!(stream.withdrawn, 200);
}

#[test]
fn small_payout_rounds_protocol_fee_down_to_zero() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &19);
    s.env.ledger().with_mut(|l| l.timestamp += 1);

    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 19);
    assert_eq!(s.token.balance(&s.ngo), 19);
    assert_eq!(s.token.balance(&treasury), 0);
    assert_eq!(s.client.get_stream(&stream_id).withdrawn, 19);
}

#[test]
fn protocol_fee_becomes_nonzero_at_the_rounding_boundary() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &20);
    s.env.ledger().with_mut(|l| l.timestamp += 1);

    let withdrawn = s.client.withdraw(&stream_id);
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
fn set_fee_bps_rejects_over_cap() {
    let s = setup();
    let result = s.client.try_set_fee_bps(&1_001);
    assert_eq!(result, Err(Ok(Error::FeeTooHigh)));
}

#[test]
fn set_fee_bps_boundary_exact_max_succeeds() {
    let s = setup();
    s.client.set_treasury(&Address::generate(&s.env));

    s.client.set_fee_bps(&1_000);
    assert_eq!(s.client.fee_bps(), 1_000);

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

    s.client.set_fee_bps(&500);
    assert_last_event(&s.env, (symbol_short!("feeset"),), 500u32);
    assert_eq!(s.client.fee_bps(), 500);

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

// ── Per-token fee overrides ───────────────────────────────────────────────────

#[test]
fn token_fee_bps_falls_back_to_the_global_default_with_no_override() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);

    assert_eq!(s.client.token_fee_bps(&s.token.address), 500);

    let other_token = Address::generate(&s.env);
    assert_eq!(s.client.token_fee_bps(&other_token), 500);
}

#[test]
fn token_fee_bps_override_takes_priority_over_the_global_default() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);
    s.client.set_token_fee_bps(&s.token.address, &100);

    assert_eq!(s.client.token_fee_bps(&s.token.address), 100);

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
    s.client.set_fee_bps(&500);
    s.client.set_token_fee_bps(&s.token.address, &100);

    assert_eq!(s.client.token_fee_bps(&s.token.address), 100);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);
    let net = s.client.withdraw(&stream_id);
    assert_eq!(net, 495);
    assert_eq!(s.token.balance(&s.ngo), 495);
    assert_eq!(s.token.balance(&treasury), 5);

    s.client.clear_token_fee_bps(&s.token.address);
    assert_eq!(s.client.token_fee_bps(&s.token.address), 500);

    s.env.ledger().with_mut(|l| l.timestamp += 50);
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
    s.client.set_fee_bps(&500);
    s.client.set_token_fee_bps(&s.token.address, &0);

    assert_eq!(s.client.token_fee_bps(&s.token.address), 0);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let net = s.client.withdraw(&stream_id);
    assert_eq!(net, 500);
    assert_eq!(s.token.balance(&s.ngo), 500);
    assert_eq!(s.token.balance(&treasury), 0);

    s.client.clear_token_fee_bps(&s.token.address);
    assert_eq!(s.client.token_fee_bps(&s.token.address), 500);

    s.env.ledger().with_mut(|l| l.timestamp += 50);
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
    s.client.set_fee_bps(&500);
    s.client.set_token_fee_bps(&s.token.address, &100);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let (net, fee) = s.client.pending_payout(&stream_id);
    assert_eq!(fee, 5);
    assert_eq!(net, 495);

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

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let (net, fee) = s.client.pending_payout(&stream_id);
    assert_eq!(fee, 0);
    assert_eq!(net, 500);

    assert_eq!(s.token.balance(&s.ngo), 0);
    assert_eq!(s.token.balance(&treasury), 0);
}

#[test]
fn pending_payout_with_nonzero_fee_matches_actual_withdraw_split() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let (net, fee) = s.client.pending_payout(&stream_id);
    assert_eq!(fee, 25);
    assert_eq!(net, 475);

    assert_eq!(s.token.balance(&s.ngo), 0);
    assert_eq!(s.token.balance(&treasury), 0);

    s.client.withdraw(&stream_id);
    assert_eq!(s.token.balance(&s.ngo), net);
    assert_eq!(s.token.balance(&treasury), fee);
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
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let (net, fee) = s.client.pending_payout(&stream_id);
    assert_eq!(fee, 0);
    assert_eq!(net, 500);

    assert_eq!(s.token.balance(&s.ngo), 0);
    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.balance, 1_000);
    assert_eq!(stream.withdrawn, 0);
}

#[test]
fn withdraw_event_amounts_with_non_zero_fee() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let net = s.client.withdraw(&stream_id);
    let withdrew = last_event(&s.env);

    let gross: i128 = 500;
    let fee: i128 = 25;
    assert_eq!(net, 475);
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
    s.client.set_fee_bps(&500);
    s.client.set_token_fee_bps(&s.token.address, &200);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let net = s.client.withdraw(&stream_id);
    let withdrew = last_event(&s.env);

    let gross: i128 = 500;
    let fee: i128 = 10;
    assert_eq!(net, 490);
    assert_eq!(net + fee, gross);

    assert_eq!(
        withdrew,
        (
            (symbol_short!("withdraw"), stream_id).into_val(&s.env),
            (gross, fee, net, s.ngo.clone()).into_val(&s.env),
        )
    );
}

#[test]
fn withdraw_event_amounts_with_zero_fee() {
    let s = setup();
    s.token_admin.mint(&s.donor, &2_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let net = s.client.withdraw(&stream_id);
    let withdrew = last_event(&s.env);

    let gross: i128 = 500;
    let fee: i128 = 0;
    assert_eq!(net, 500);
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
fn token_fee_override_survives_instance_ttl_advance() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    let treasury = Address::generate(&s.env);
    s.client.set_treasury(&treasury);
    s.client.set_fee_bps(&500);
    s.client.set_token_fee_bps(&s.token.address, &100);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

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
    assert_eq!(fee, 5, "pending_payout uses the 1% override");
    assert_eq!(net, 495);

    let withdrawn = s.client.withdraw(&stream_id);
    assert_eq!(withdrawn, 495, "withdraw uses the 1% override");
    assert_eq!(s.token.balance(&treasury), 5);
    assert_eq!(s.token.balance(&s.ngo), 495);
}

// ── fee_from_payout helper ────────────────────────────────────────────────────

#[test]
fn fee_from_payout_returns_accrued_minus_net() {
    assert_eq!(fee_from_payout(1_000, 950), Ok(50));
    assert_eq!(fee_from_payout(1_000, 1_000), Ok(0));
}

#[test]
fn fee_from_payout_reports_overflow_instead_of_zero() {
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
    s.client.set_fee_bps(&500);
    s.token_admin.mint(&s.donor, &1_000);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    s.env.ledger().with_mut(|l| l.timestamp += 50);

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

// ── Payout address ────────────────────────────────────────────────────────────

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

    s.env.ledger().with_mut(|l| l.timestamp += 50);

    let net = s.client.withdraw(&stream_id);

    assert_eq!(net, 500);
    assert_eq!(s.token.balance(&payout), 500);
    assert_eq!(s.token.balance(&s.ngo), 0);

    let withdrew = last_event(&s.env);
    assert_eq!(
        withdrew,
        (
            (symbol_short!("withdraw"), stream_id).into_val(&s.env),
            (500i128, 0i128, 500i128, payout.clone()).into_val(&s.env),
        )
    );
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

    s.client.set_payout_address(&s.ngo, &first);
    s.env.ledger().with_mut(|l| l.timestamp += 50);
    assert_eq!(s.client.withdraw(&stream_id), 500);
    assert_eq!(s.token.balance(&first), 500);

    s.client.set_payout_address(&s.ngo, &second);
    s.env.ledger().with_mut(|l| l.timestamp += 50);
    assert_eq!(s.client.withdraw(&stream_id), 500);

    assert_eq!(s.token.balance(&second), 500);
    assert_eq!(s.token.balance(&first), 500);
    assert_eq!(s.token.balance(&s.ngo), 0);
}

// ── Surplus / committed balance ───────────────────────────────────────────────

#[test]
fn committed_balance_tracks_deposits_and_accrued_withdrawals() {
    let s = setup();
    s.token_admin.mint(&s.donor, &1_000);

    assert_eq!(s.client.committed_balance(&s.token.address), 0);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    assert_eq!(s.client.committed_balance(&s.token.address), 1_000);
    assert_eq!(s.token.balance(&s.client.address), 1_000);
    assert_eq!(s.client.surplus(&s.token.address), 0);

    s.env.ledger().with_mut(|l| l.timestamp += 40);
    s.client.withdraw(&stream_id);

    assert_eq!(s.client.committed_balance(&s.token.address), 600);
    assert_eq!(s.token.balance(&s.client.address), 600);
    assert_eq!(s.client.surplus(&s.token.address), 0);
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

    s.env.ledger().with_mut(|l| l.timestamp += 20);
    s.client.cancel_stream(&stream_id);

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
    s.token.transfer(&s.donor, &s.client.address, &400);

    let result = s.client.try_sweep_surplus(&s.token.address, &401);
    assert_eq!(result, Err(Ok(Error::SweepExceedsSurplus)));

    let result = s.client.try_sweep_surplus(&s.token.address, &1_400);
    assert_eq!(result, Err(Ok(Error::SweepExceedsSurplus)));

    s.client.sweep_surplus(&s.token.address, &400);
    assert_eq!(s.token.balance(&s.client.address), 1_000);

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

// ── Configuration events ──────────────────────────────────────────────────────

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

#[test]
fn set_min_duration_publishes_event() {
    let s = setup();

    s.client.set_min_duration(&60);

    assert_last_event(&s.env, (symbol_short!("mindur"),), 60u64);
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
