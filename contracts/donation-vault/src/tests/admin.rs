// SPDX-License-Identifier: Apache-2.0
//! Tests for admin transfer, renounce, upgrade (immediate and timelocked),
//! registry configuration, get_config, and related admin-only operations.

use soroban_sdk::testutils::{Ledger, MockAuth, MockAuthInvoke};
use soroban_sdk::{symbol_short, Bytes, BytesN, IntoVal};

use crate::tests::helpers::*;
use crate::*;

// ── Admin transfer ────────────────────────────────────────────────────────────

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
fn accept_admin_without_proposal_fails() {
    let s = setup();
    let result = s.client.try_accept_admin();
    assert_eq!(result, Err(Ok(Error::NoPendingAdmin)));
}

#[test]
fn repropose_admin_overwrites_earlier_proposal() {
    let s = setup();
    let old_admin = s.client.admin();
    let admin_a = Address::generate(&s.env);
    let admin_b = Address::generate(&s.env);

    s.client.propose_admin(&admin_a);
    assert_eq!(s.client.pending_admin(), Some(admin_a.clone()));

    s.client.propose_admin(&admin_b);
    assert_eq!(
        s.client.pending_admin(),
        Some(admin_b.clone()),
        "the second proposal must overwrite the first"
    );

    s.client.accept_admin();
    assert_auth_required_from(&s, &admin_b, "accept_admin");

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
fn accept_admin_requires_pending_admin_auth() {
    let s = setup();
    let new_admin = Address::generate(&s.env);
    s.client.propose_admin(&new_admin);

    s.client.accept_admin();

    assert_auth_required_from(&s, &new_admin, "accept_admin");
}

// ── Renounce ──────────────────────────────────────────────────────────────────

#[test]
fn renounce_admin_clears_admin_and_pending_proposal() {
    let s = setup();
    let admin = s.client.admin();
    let new_admin = Address::generate(&s.env);

    s.client.propose_admin(&new_admin);
    assert_eq!(s.client.pending_admin(), Some(new_admin.clone()));

    s.client.renounce_admin();
    assert_auth_required_from(&s, &admin, "renounce_admin");

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

    assert_eq!(s.client.try_admin(), Err(Ok(Error::AdminRenounced)));
    assert_eq!(
        s.client.try_init(&candidate),
        Err(Ok(Error::AdminRenounced))
    );
    assert_eq!(
        s.client.try_propose_admin(&candidate),
        Err(Ok(Error::AdminRenounced))
    );
    assert_eq!(s.client.try_pause(), Err(Ok(Error::AdminRenounced)));
}

#[test]
fn renounce_admin_without_admin_fails() {
    let s = setup();
    s.client.renounce_admin();

    assert_eq!(
        s.client.try_renounce_admin(),
        Err(Ok(Error::AdminRenounced))
    );
}

// ── Admin auth guards ─────────────────────────────────────────────────────────

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
fn admin_writes_bump_instance_ttl() {
    let s = setup();
    assert_eq!(instance_ttl(&s), INSTANCE_BUMP_AMOUNT);

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

// ── get_config ────────────────────────────────────────────────────────────────

#[test]
fn get_config_reflects_admin_settings() {
    let s = setup();
    s.token_admin.mint(&s.donor, &5_000);

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
        }
    );

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
            registry: None,
            cancel_grace_ledgers: 10,
            total_donated: 0,
            total_withdrawn: 0,
            total_fees_collected: 0,
            pending_admin: None,
            renounced: false,
        }
    );

    let candidate = Address::generate(&s.env);
    s.client.propose_admin(&candidate);
    assert_eq!(s.client.get_config().pending_admin, Some(candidate));

    s.client.set_fee_bps(&250);
    assert_eq!(s.client.get_config().fee_bps, 250);

    s.client.unpause();
    assert!(!s.client.get_config().paused);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(s.client.get_config().total_donated, 1_000);

    s.client.top_up(&stream_id, &500);
    assert_eq!(s.client.get_config().total_donated, 1_500);

    s.client.set_registry(&registry);

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
fn admin_functions_before_init_return_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(DonationVault, DonationVaultArgs::__constructor(&admin));
    let client = DonationVaultClient::new(&env, &contract_id);

    env.as_contract(&contract_id, || {
        env.storage().instance().remove(&DataKey::Admin);
    });

    let treasury = Address::generate(&env);
    let new_admin = Address::generate(&env);

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
    assert_eq!(
        client.try_admin(),
        Err(Ok(Error::NotInitialized)),
        "admin() must return NotInitialized before init"
    );
    assert_eq!(
        client.try_accept_admin(),
        Err(Ok(Error::NoPendingAdmin)),
        "accept_admin() must fail closed before init"
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
fn constructor_publishes_init_event() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    env.register(DonationVault, DonationVaultArgs::__constructor(&admin));

    assert_last_event(&env, (symbol_short!("init"),), admin.clone());
}

// ── max_streams_per_donor ─────────────────────────────────────────────────────

#[test]
fn max_streams_per_donor_defaults_to_100() {
    let s = setup();
    assert_eq!(s.client.max_streams_per_donor(), 100);
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

// ── max_rate ──────────────────────────────────────────────────────────────────

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

// ── min_duration ──────────────────────────────────────────────────────────────

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
fn allowed_tokens_is_empty_when_unconfigured() {
    let s = setup();
    assert_eq!(s.client.allowed_tokens().len(), 0);
}

// ── Upgrade (immediate) ───────────────────────────────────────────────────────

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

    let unuploaded_hash = BytesN::from_array(&s.env, &[99u8; 32]);
    assert!(s.client.try_upgrade(&unuploaded_hash).is_err());

    let config_after = s.client.get_config();
    let stream_after = s.client.get_stream(&stream_id);
    assert_eq!(config_before, config_after);
    assert_eq!(stream_before, stream_after);
}

#[test]
fn upgrade_publishes_event() {
    let s = setup();
    let new_wasm_hash = s.env.deployer().upload_contract_wasm(Bytes::new(&s.env));

    s.client.upgrade(&new_wasm_hash);

    assert_last_event(&s.env, (symbol_short!("upgrade"),), new_wasm_hash.clone());
}

// ── Timelocked upgrade ────────────────────────────────────────────────────────

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
    s.client.set_upgrade_delay(&0);
    let new_wasm_hash = s.env.deployer().upload_contract_wasm(Bytes::new(&s.env));

    s.client.upgrade(&new_wasm_hash);

    assert_last_event(&s.env, (symbol_short!("upgrade"),), new_wasm_hash.clone());
}

// ── Registry ──────────────────────────────────────────────────────────────────

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

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));
    assert_eq!(s.token.balance(&s.donor), 1_000);
}

#[test]
fn create_stream_fails_for_unregistered_ngo() {
    let (s, _registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));
    assert_eq!(s.token.balance(&s.donor), 1_000);
}

#[test]
fn create_stream_fails_for_revoked_ngo() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

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

    let bad_registry = Address::generate(&s.env);
    s.client.set_registry(&bad_registry);
    assert_eq!(s.client.registry(), Some(bad_registry));

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    s.client.clear_registry();
    assert_eq!(s.client.registry(), None);

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
fn set_registry_publishes_event() {
    let s = setup();
    let registry = Address::generate(&s.env);

    s.client.set_registry(&registry);

    assert_last_event(&s.env, (symbol_short!("regset"),), registry.clone());
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

    s.client.set_registry(&registry_id);
    assert_eq!(s.client.registry(), Some(registry_id.clone()));

    s.client.clear_registry();
    assert_eq!(s.client.registry(), None);

    s.client.set_registry(&registry_id);
    assert_eq!(s.client.registry(), Some(registry_id.clone()));

    let result = s
        .client
        .try_create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

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

    registry.revoke_ngo(&s.client.admin(), &s.ngo);

    let donor_balance_before = s.token.balance(&s.donor);
    let stream_balance_before = s.client.get_stream(&stream_id).balance;
    let total_donated_before = s.client.get_config().total_donated;

    let result = s.client.try_top_up(&stream_id, &500);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

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

    registry.revoke_ngo(&s.client.admin(), &s.ngo);
    assert_eq!(
        s.client.try_top_up(&stream_id, &500),
        Err(Ok(Error::NgoNotVerified))
    );

    registry.approve_ngo(&s.client.admin(), &s.ngo);

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

    registry.revoke_ngo(&s.client.admin(), &s.ngo);

    s.env.ledger().with_mut(|l| l.timestamp += 20);

    s.client.withdraw(&stream_id);
    assert_eq!(s.token.balance(&s.ngo), 200);

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

    let bad_registry = Address::generate(&s.env);
    s.client.set_registry(&bad_registry);

    let result = s.client.try_top_up(&stream_id, &500);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    s.client.clear_registry();
    s.client.top_up(&stream_id, &500);
    assert_eq!(s.client.get_stream(&stream_id).balance, 1_500);
}

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
    s.env.ledger().with_mut(|l| l.timestamp += 10);

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

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    let result = s.client.try_modify_rate(&stream_id, &20);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.rate, 10);
    assert_eq!(stream.balance, 1_000);
}

#[test]
fn modify_rate_decrease_allows_unverified_ngo() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    s.client.modify_rate(&stream_id, &5);
    assert_eq!(s.client.get_stream(&stream_id).rate, 5);
}

#[test]
fn modify_rate_increase_rejects_revoked_ngo() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    registry.revoke_ngo(&s.client.admin(), &s.ngo);

    let result = s.client.try_modify_rate(&stream_id, &20);
    assert_eq!(result, Err(Ok(Error::NgoNotVerified)));

    let stream = s.client.get_stream(&stream_id);
    assert_eq!(stream.rate, 10);
    assert_eq!(stream.balance, 1_000);
}

#[test]
fn modify_rate_decrease_allows_revoked_ngo() {
    let (s, registry) = setup_with_registry();
    s.token_admin.mint(&s.donor, &1_000);

    registry.register(&s.ngo, &soroban_sdk::String::from_str(&s.env, "Red Cross"));
    registry.approve_ngo(&s.client.admin(), &s.ngo);

    let stream_id = s
        .client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10);

    registry.revoke_ngo(&s.client.admin(), &s.ngo);

    s.client.modify_rate(&stream_id, &5);
    assert_eq!(s.client.get_stream(&stream_id).rate, 5);
}
