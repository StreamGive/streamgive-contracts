// SPDX-License-Identifier: Apache-2.0
//! Tests for admin-only operations: initialisation, contract upgrades and the
//! upgrade timelock, pausing registrations, the approver list, the
//! verification period, `remove_ngo`, and the roles documentation check.

use super::helpers::*;
use crate::*;
use soroban_sdk::testutils::{
    Address as _, AuthorizedFunction, Events as _, Ledger, MockAuth, MockAuthInvoke,
};
use soroban_sdk::{symbol_short, vec, IntoVal, Symbol, TryFromVal};

// ── Initialisation ────────────────────────────────────────────────────────────

#[test]
fn init_sets_admin() {
    let (_env, client, admin) = setup();
    assert_eq!(client.admin(), admin);
}

#[test]
fn double_init_fails() {
    let (_env, client, admin) = setup();
    let result = client.try_init(&admin);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
#[should_panic]
fn init_without_admin_auth_fails() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    let client = NgoRegistryClient::new(&env, &contract_id);

    let attacker = Address::generate(&env);
    env.mock_auths(&[]);
    client.init(&attacker);
}

#[test]
fn constructor_publishes_init_event() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    // The constructor is the latest top-level invocation, so its event is the
    // only contract event in the buffer.
    let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("init"),).into_val(&env),
                admin.into_val(&env),
            ),
        ]
    );
}

// ── Contract upgrade and timelock ─────────────────────────────────────────────

#[test]
#[ignore = "requires an uploaded wasm hash; upgrade panics before auth is observable"]
fn upgrade_requires_admin_auth() {
    let (env, client, admin) = setup();
    let fake_hash = BytesN::from_array(&env, &[1u8; 32]);

    client.upgrade(&fake_hash);
    let auths = env.auths();
    assert_eq!(auths.len(), 1, "expected exactly one authorizer");
    assert_eq!(auths[0].0, admin);
    match &auths[0].1.function {
        AuthorizedFunction::Contract((contract, function, _)) => {
            assert_eq!(contract, &client.address);
            assert_eq!(function, &Symbol::new(&env, "upgrade"));
        }
        _ => panic!("expected a contract invocation"),
    }
}

#[test]
fn upgrade_rejects_non_admin() {
    let (env, client, _admin) = setup();
    let fake_hash = BytesN::from_array(&env, &[1u8; 32]);

    env.mock_auths(&[]);
    let result = client.try_upgrade(&fake_hash);
    assert!(result.is_err());
}

#[test]
fn upgrade_fails_with_unuploaded_wasm_hash() {
    let (env, client, _admin) = setup();
    let unuploaded_hash = BytesN::from_array(&env, &[42u8; 32]);
    let result = client.try_upgrade(&unuploaded_hash);
    assert!(result.is_err());
}

#[test]
fn upgrade_preserves_state() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Doctors Without Borders");

    client.register(&owner, &name);
    client.approve_ngo(&admin, &owner);

    let ngo_before = client.get_ngo(&owner);
    assert!(ngo_before.verified);
    assert_eq!(ngo_before.name, name);
    assert_eq!(client.ngo_count(), 1);

    // Negative case: attempting an upgrade with an unuploaded wasm hash fails
    let unuploaded_hash = BytesN::from_array(&env, &[99u8; 32]);
    assert!(client.try_upgrade(&unuploaded_hash).is_err());

    // State reads back identically after
    let ngo_after = client.get_ngo(&owner);
    assert_eq!(ngo_before.owner, ngo_after.owner);
    assert_eq!(ngo_before.name, ngo_after.name);
    assert_eq!(ngo_before.verified, ngo_after.verified);
    assert_eq!(client.ngo_count(), 1);
}

#[test]
fn upgrade_publishes_event() {
    let (env, client, _admin) = setup();
    // Upload any bytes so the upgrade target exists; the host only checks
    // that the hash is present, not that it is executable.
    let new_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

    client.upgrade(&new_wasm_hash);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("upgrade"),).into_val(&env),
                new_wasm_hash.into_val(&env),
            ),
        ]
    );
}

#[test]
fn upgrade_with_timelock_disabled_succeeds() {
    let (env, client, _admin) = setup();
    let new_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

    // Default upgrade_delay is 0, so immediate upgrade should work
    client.upgrade(&new_wasm_hash);
    assert_only_event(
        &env,
        &client,
        (symbol_short!("upgrade"),),
        new_wasm_hash.clone(),
    );
}

#[test]
fn upgrade_with_timelock_enabled_fails() {
    let (env, client, _admin) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.set_upgrade_delay(&3_600);

    let new_wasm_hash = BytesN::from_array(&env, &[7u8; 32]);
    let result = client.try_upgrade(&new_wasm_hash);

    assert_eq!(result, Err(Ok(Error::UpgradeTimelockEnabled)));
}

#[test]
fn set_upgrade_delay_stores_delay_and_publishes_event() {
    let (env, client, _admin) = setup();

    client.set_upgrade_delay(&3_600);

    assert_only_event(&env, &client, (symbol_short!("updelay"),), 3_600u64);

    assert_eq!(client.upgrade_delay(), 3_600);
}

#[test]
fn propose_upgrade_records_pending_and_publishes_event() {
    let (env, client, _admin) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.set_upgrade_delay(&3_600);

    let new_wasm_hash = BytesN::from_array(&env, &[7u8; 32]);

    client.propose_upgrade(&new_wasm_hash);

    assert_only_event(
        &env,
        &client,
        (symbol_short!("uppropose"), new_wasm_hash.clone()),
        4_600u64,
    );

    let pending = client.pending_upgrade().expect("a proposal is pending");
    assert_eq!(pending.new_wasm_hash, new_wasm_hash);
    assert_eq!(pending.executable_at, 4_600);
}

#[test]
fn propose_upgrade_replaces_previous_proposal() {
    let (env, client, _admin) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.set_upgrade_delay(&3_600);

    let first = BytesN::from_array(&env, &[1u8; 32]);
    let second = BytesN::from_array(&env, &[2u8; 32]);

    client.propose_upgrade(&first);
    env.ledger().with_mut(|l| l.timestamp = 1_500);
    client.propose_upgrade(&second);

    let pending = client.pending_upgrade().expect("a proposal is pending");
    assert_eq!(pending.new_wasm_hash, second);
    assert_eq!(pending.executable_at, 5_100);
}

#[test]
fn propose_upgrade_requires_admin() {
    let (env, client, _admin) = setup();
    let new_wasm_hash = BytesN::from_array(&env, &[7u8; 32]);

    env.mock_auths(&[]);
    assert!(client.try_propose_upgrade(&new_wasm_hash).is_err());
}

#[test]
fn execute_upgrade_before_delay_elapsed_fails() {
    let (env, client, _admin) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.set_upgrade_delay(&3_600);

    let new_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

    client.propose_upgrade(&new_wasm_hash);

    let result = client.try_execute_upgrade();
    assert_eq!(result, Err(Ok(Error::UpgradeDelayNotElapsed)));
}

#[test]
fn execute_upgrade_after_delay_elapsed_succeeds() {
    let (env, client, _admin) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.set_upgrade_delay(&3_600);

    let new_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

    client.propose_upgrade(&new_wasm_hash);
    env.ledger().with_mut(|l| l.timestamp = 4_600);

    client.execute_upgrade();

    assert_only_event(
        &env,
        &client,
        (symbol_short!("upgrade"),),
        new_wasm_hash.clone(),
    );
    assert!(client.pending_upgrade().is_none());
}

#[test]
fn execute_upgrade_without_proposal_fails() {
    let (_env, client, _admin) = setup();

    let result = client.try_execute_upgrade();
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
}

#[test]
fn execute_upgrade_requires_admin() {
    let (env, client, _admin) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.set_upgrade_delay(&1);
    let new_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
    client.propose_upgrade(&new_wasm_hash);
    env.ledger().with_mut(|l| l.timestamp = 1_001);

    env.mock_auths(&[]);
    assert!(client.try_execute_upgrade().is_err());
}

#[test]
fn cancel_upgrade_clears_pending_and_publishes_event() {
    let (env, client, _admin) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.set_upgrade_delay(&3_600);

    let new_wasm_hash = BytesN::from_array(&env, &[9u8; 32]);

    client.propose_upgrade(&new_wasm_hash);
    client.cancel_upgrade();
    assert_only_event(
        &env,
        &client,
        (symbol_short!("upcancel"),),
        new_wasm_hash.clone(),
    );

    assert!(client.pending_upgrade().is_none());
}

#[test]
fn cancel_upgrade_without_proposal_fails() {
    let (_env, client, _admin) = setup();

    let result = client.try_cancel_upgrade();
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
}

#[test]
fn cancel_upgrade_requires_admin() {
    let (env, client, _admin) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.set_upgrade_delay(&3_600);
    let new_wasm_hash = BytesN::from_array(&env, &[9u8; 32]);
    client.propose_upgrade(&new_wasm_hash);

    env.mock_auths(&[]);
    assert!(client.try_cancel_upgrade().is_err());
}

#[test]
fn upgrade_preserves_ngo_entries_after_timelocked_upgrade() {
    let (env, client, admin) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");
    client.register(&owner, &name);
    client.approve_ngo(&admin, &owner);

    client.set_upgrade_delay(&100);
    let new_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
    client.propose_upgrade(&new_wasm_hash);

    env.ledger().with_mut(|l| l.timestamp = 1_100);
    client.execute_upgrade();

    let ngo = client.get_ngo(&owner);
    assert_eq!(ngo.name, name);
    assert!(ngo.verified);
}

// ── Registration pause ────────────────────────────────────────────────────────

#[test]
fn registrations_paused_is_false_by_default() {
    let (_env, client, _admin) = setup();
    assert!(!client.registrations_paused());
}

#[test]
fn pause_registrations_blocks_register() {
    let (env, client, _admin) = setup();

    client.pause_registrations();
    assert!(client.registrations_paused());

    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");
    assert_eq!(
        client.try_register(&owner, &name),
        Err(Ok(Error::RegistrationsPaused))
    );
    // No entry should have been stored
    assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
    assert_eq!(client.ngo_count(), 0);
}

#[test]
fn resume_registrations_unblocks_register() {
    let (env, client, _admin) = setup();

    client.pause_registrations();
    assert!(client.registrations_paused());

    client.resume_registrations();
    assert!(!client.registrations_paused());

    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Re-opened NGO");
    client.register(&owner, &name);
    assert_eq!(client.ngo_count(), 1);
    assert!(!client.get_ngo(&owner).verified);
}

#[test]
fn pause_registrations_does_not_affect_other_functions() {
    let (env, client, admin) = setup();

    // Register one NGO, then pause registrations
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Pre-pause NGO");
    client.register(&owner, &name);

    client.pause_registrations();
    assert!(client.registrations_paused());

    // All non-register entry points must still work
    // approve_ngo still works
    client.approve_ngo(&admin, &owner);
    assert!(client.is_verified(&owner));

    // revoke_ngo still works
    client.revoke_ngo(&admin, &owner);
    assert!(!client.is_verified(&owner));

    // read-only views are unaffected
    assert_eq!(client.ngo_count(), 1);
    let ngo = client.get_ngo(&owner);
    assert_eq!(ngo.name, name);
}

#[test]
fn pause_registrations_emits_event() {
    let (env, client, _admin) = setup();
    client.pause_registrations();

    let all = env.events().all();
    let events = all.events();
    let last = &events[events.len() - 1];
    let soroban_sdk::xdr::ContractEventBody::V0(body) = &last.body;
    let first_topic = body.topics.first().expect("at least one topic");
    let expected: soroban_sdk::xdr::ScVal =
        soroban_sdk::xdr::ScVal::try_from_val(&env, &symbol_short!("regrpause").to_val()).unwrap();
    assert_eq!(first_topic, &expected);
}

#[test]
fn resume_registrations_emits_event() {
    let (env, client, _admin) = setup();
    client.pause_registrations();
    client.resume_registrations();

    let all = env.events().all();
    let events = all.events();
    let last = &events[events.len() - 1];
    let soroban_sdk::xdr::ContractEventBody::V0(body) = &last.body;
    let first_topic = body.topics.first().expect("at least one topic");
    let expected: soroban_sdk::xdr::ScVal =
        soroban_sdk::xdr::ScVal::try_from_val(&env, &symbol_short!("regresume").to_val()).unwrap();
    assert_eq!(first_topic, &expected);
}

#[test]
fn pause_registrations_idempotent_error() {
    let (_env, client, _admin) = setup();
    client.pause_registrations();
    // Calling pause again while already paused should fail
    assert_eq!(
        client.try_pause_registrations(),
        Err(Ok(Error::RegistrationsPaused))
    );
}

#[test]
fn resume_registrations_when_not_paused_errors() {
    let (_env, client, _admin) = setup();
    // Not paused yet — should return an error
    assert!(client.try_resume_registrations().is_err());
}

#[test]
#[should_panic]
fn pause_registrations_requires_admin_auth() {
    let (env, client, _admin) = setup();
    env.mock_auths(&[]);
    client.pause_registrations();
}

// ── Approver list ─────────────────────────────────────────────────────────────

#[test]
fn list_approvers_is_empty_by_default() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.list_approvers().len(), 0);
}

#[test]
fn add_approver_lists_approver_and_publishes_event() {
    let (env, client, _admin) = setup();
    let reviewer = Address::generate(&env);

    client.add_approver(&reviewer);

    // Read the events before any further contract call replaces them.
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("apadd"), reviewer.clone()).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );
    assert!(client.is_approver(&reviewer));
    assert_eq!(client.list_approvers(), vec![&env, reviewer.clone()]);
}

#[test]
fn add_approver_twice_fails() {
    let (env, client, _admin) = setup();
    let reviewer = Address::generate(&env);
    client.add_approver(&reviewer);

    assert_eq!(
        client.try_add_approver(&reviewer),
        Err(Ok(Error::AlreadyApprover))
    );
    assert_eq!(client.list_approvers().len(), 1);
}

#[test]
fn remove_approver_unlists_approver_and_publishes_event() {
    let (env, client, _admin) = setup();
    let reviewer = Address::generate(&env);
    client.add_approver(&reviewer);

    client.remove_approver(&reviewer);

    // Read the events before any further contract call replaces them.
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("aprem"), reviewer.clone()).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );
    assert!(!client.is_approver(&reviewer));
    assert_eq!(client.list_approvers().len(), 0);
}

#[test]
fn remove_unlisted_approver_fails() {
    let (env, client, _admin) = setup();
    let reviewer = Address::generate(&env);

    assert_eq!(
        client.try_remove_approver(&reviewer),
        Err(Ok(Error::NotApprover))
    );
}

#[test]
#[should_panic]
fn add_approver_without_admin_auth_fails() {
    let (env, client, _admin) = setup();
    let reviewer = Address::generate(&env);

    env.mock_auths(&[]);
    client.add_approver(&reviewer);
}

#[test]
#[should_panic]
fn remove_approver_without_admin_auth_fails() {
    let (env, client, _admin) = setup();
    let reviewer = Address::generate(&env);
    client.add_approver(&reviewer);

    env.mock_auths(&[]);
    client.remove_approver(&reviewer);
}

#[test]
fn approver_does_not_gain_admin_rights() {
    let (env, client, _admin) = setup();
    let reviewer = Address::generate(&env);
    let owner = Address::generate(&env);
    client.add_approver(&reviewer);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    // Only the approver authorizes the next call. An admin-only operation
    // (`remove_ngo`) must still fail: review rights are not admin rights.
    env.mock_auths(&[MockAuth {
        address: &reviewer,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "remove_ngo",
            args: (owner.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert!(client.try_remove_ngo(&owner).is_err());
    assert!(!client.get_ngo(&owner).verified);
}

// ── Verification period ───────────────────────────────────────────────────────

#[test]
fn verify_period_defaults_to_zero() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.verify_period(), 0);
}

#[test]
fn set_verify_period_stores_value_and_emits_event() {
    let (env, client, _admin) = setup();
    let period = 1_000u64;
    client.set_verify_period(&period);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("verperiod"),).into_val(&env),
                period.into_val(&env),
            ),
        ]
    );
    assert_eq!(client.verify_period(), period);
}

#[test]
fn set_verify_period_requires_admin_auth() {
    let (env, client, admin) = setup();
    client.set_verify_period(&1_000);

    assert_sole_auth(&env, &client, &admin, "set_verify_period");
}

#[test]
fn set_verify_period_does_not_change_existing_approvals() {
    let (env, client, admin) = setup();
    client.set_verify_period(&1_000);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    assert_eq!(client.get_ngo(&owner).verified_until, Some(1_000));

    // Raising the period only affects future approvals and renewals.
    client.set_verify_period(&5_000);
    assert_eq!(client.get_ngo(&owner).verified_until, Some(1_000));
}

// ── remove_ngo ────────────────────────────────────────────────────────────────

#[test]
fn remove_ngo_deletes_unverified_entry() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    assert_eq!(client.total_ngos(), 1);

    client.remove_ngo(&owner);

    assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
    assert!(!client.is_verified(&owner));
    assert_eq!(client.total_ngos(), 0);
    assert_eq!(client.list_ngos(&0, &10).len(), 0);
}

#[test]
fn remove_ngo_publishes_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.remove_ngo(&owner);

    // Events are cleared per top-level call; only the removed event is visible.
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("removed"), owner).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );
}

#[test]
fn remove_ngo_rejects_a_verified_ngo() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    let result = client.try_remove_ngo(&owner);

    assert_eq!(result, Err(Ok(Error::AlreadyVerified)));
    // Still there, still verified: revoke_ngo is the way to undo an approval.
    assert!(client.get_ngo(&owner).verified);
    assert_eq!(client.total_ngos(), 1);
}

#[test]
fn remove_ngo_after_revocation_succeeds() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    client.revoke_ngo(&admin, &owner);

    client.remove_ngo(&owner);

    assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
    assert_eq!(client.total_ngos(), 0);
}

#[test]
fn remove_ngo_unknown_owner_fails() {
    let (env, client, _admin) = setup();
    let unknown = Address::generate(&env);

    let result = client.try_remove_ngo(&unknown);

    assert_eq!(result, Err(Ok(Error::NotRegistered)));
}

#[test]
fn remove_ngo_requires_admin_auth() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.remove_ngo(&owner);

    assert_sole_auth(&env, &client, &admin, "remove_ngo");
}

#[test]
#[should_panic]
fn remove_ngo_without_admin_auth_fails() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    env.mock_auths(&[]);
    client.remove_ngo(&owner);
}

#[test]
fn remove_ngo_frees_the_index_and_lets_the_owner_register_again() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    client.register(&a, &String::from_str(&env, "A"));
    client.register(&b, &String::from_str(&env, "B"));
    client.register(&c, &String::from_str(&env, "C"));

    // Removing the middle entry swaps the last one into its slot.
    client.remove_ngo(&b);

    let owners = client.list_ngos(&0, &10);
    assert_eq!(owners.len(), 2);
    assert_eq!(owners.get(0).unwrap(), a);
    assert_eq!(owners.get(1).unwrap(), c);
    assert_eq!(client.total_ngos(), 2);

    // The freed owner can apply again from scratch.
    client.register(&b, &String::from_str(&env, "B again"));
    assert!(!client.get_ngo(&b).verified);
    assert_eq!(client.total_ngos(), 3);
}

// ── Roles documentation ───────────────────────────────────────────────────────

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
    const SOURCE: &str = include_str!("../lib.rs");
    const ROLES: &str = include_str!("../../../../docs/ROLES.md");
    const IMPL: &str = "impl NgoRegistry {";
    const START: &str = "<!-- roles:ngo-registry:start -->";
    const END: &str = "<!-- roles:ngo-registry:end -->";

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
            None => {
                panic!("`{name}` is a public NgoRegistry entry point with no row in docs/ROLES.md")
            }
            Some((_, role)) => assert!(
                !role.is_empty(),
                "`{name}` has an empty role in docs/ROLES.md"
            ),
        }
    }

    for (name, _) in role_rows(ROLES, START, END) {
        assert!(
            entry_point_names(SOURCE, IMPL).any(|entry| entry == name),
            "docs/ROLES.md lists `{name}`, which is not a public NgoRegistry entry point"
        );
        assert_eq!(
            role_rows(ROLES, START, END)
                .filter(|(row, _)| *row == name)
                .count(),
            1,
            "`{name}` appears more than once in the ngo-registry roles table"
        );
    }
}
