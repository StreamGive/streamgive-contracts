#![cfg(test)]

use super::*;
use soroban_sdk::testutils::storage::{Instance as _, Persistent as _};
use soroban_sdk::testutils::{Address as _, Ledger};

fn setup() -> (Env, NgoRegistryClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    // Leave room for the 90-day NGO bump so the TTL tests see it unclamped.
    env.ledger()
        .with_mut(|l| l.max_entry_ttl = 365 * DAY_IN_LEDGERS);

    let contract_id = env.register(NgoRegistry, ());
    let client = NgoRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    (env, client, admin)
}

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
fn register_ngo_stores_unverified_entry() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");

    client.register(&owner, &name);

    let ngo = client.get_ngo(&owner);
    assert_eq!(ngo.owner, owner);
    assert_eq!(ngo.name, name);
    assert!(!ngo.verified);
}

#[test]
fn double_register_fails() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");

    client.register(&owner, &name);
    let result = client.try_register(&owner, &name);

    assert_eq!(result, Err(Ok(Error::AlreadyRegistered)));
}

#[test]
fn get_unregistered_ngo_fails() {
    let (env, client, _admin) = setup();
    let random = Address::generate(&env);

    let result = client.try_get_ngo(&random);
    assert_eq!(result, Err(Ok(Error::NotRegistered)));
}

#[test]
fn approve_ngo_marks_verified() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");
    client.register(&owner, &name);

    client.approve_ngo(&owner);

    let ngo = client.get_ngo(&owner);
    assert!(ngo.verified);
}

#[test]
fn approve_unregistered_ngo_fails() {
    let (env, client, _admin) = setup();
    let random = Address::generate(&env);

    let result = client.try_approve_ngo(&random);
    assert_eq!(result, Err(Ok(Error::NotRegistered)));
}

#[test]
fn revoke_ngo_clears_verified_status() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");
    client.register(&owner, &name);

    client.approve_ngo(&owner);
    assert!(client.get_ngo(&owner).verified);

    client.revoke_ngo(&owner);

    let ngo = client.get_ngo(&owner);
    assert!(!ngo.verified);
}

#[test]
fn revoke_unregistered_ngo_fails() {
    let (env, client, _admin) = setup();
    let random = Address::generate(&env);

    let result = client.try_revoke_ngo(&random);
    assert_eq!(result, Err(Ok(Error::NotRegistered)));
}

#[test]
fn touch_ngo_leaves_entry_unchanged() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");
    client.register(&owner, &name);
    client.approve_ngo(&owner);

    let before = client.get_ngo(&owner);
    client.touch_ngo(&owner);
    let after = client.get_ngo(&owner);

    assert_eq!(before, after);
}

#[test]
fn touch_unregistered_ngo_fails() {
    let (env, client, _admin) = setup();
    let random = Address::generate(&env);

    let result = client.try_touch_ngo(&random);
    assert_eq!(result, Err(Ok(Error::NotRegistered)));
}

fn instance_ttl(env: &Env, client: &NgoRegistryClient) -> u32 {
    env.as_contract(&client.address, || env.storage().instance().get_ttl())
}

fn ngo_ttl(env: &Env, client: &NgoRegistryClient, owner: &Address) -> u32 {
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Ngo(owner.clone()))
    })
}

/// Moves the ledger forward two days, which drops both the instance and a
/// freshly bumped NGO entry below their lifetime thresholds. Asserts that
/// it did, so a test calling this can't pass just because nothing needed
/// a bump.
fn age_past_thresholds(env: &Env, client: &NgoRegistryClient, owner: &Address) {
    env.ledger()
        .with_mut(|l| l.sequence_number += 2 * DAY_IN_LEDGERS);
    assert!(instance_ttl(env, client) < INSTANCE_LIFETIME_THRESHOLD);
    assert!(ngo_ttl(env, client, owner) < NGO_LIFETIME_THRESHOLD);
}

fn assert_ttls_bumped(env: &Env, client: &NgoRegistryClient, owner: &Address) {
    assert_eq!(instance_ttl(env, client), INSTANCE_BUMP_AMOUNT);
    assert_eq!(ngo_ttl(env, client, owner), NGO_BUMP_AMOUNT);
}

#[test]
fn register_bumps_instance_and_ngo_ttl() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    assert_ttls_bumped(&env, &client, &owner);
}

#[test]
fn approve_ngo_bumps_instance_and_ngo_ttl() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    age_past_thresholds(&env, &client, &owner);

    client.approve_ngo(&owner);

    assert_ttls_bumped(&env, &client, &owner);
}

#[test]
fn revoke_ngo_bumps_instance_and_ngo_ttl() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&owner);
    age_past_thresholds(&env, &client, &owner);

    client.revoke_ngo(&owner);

    assert_ttls_bumped(&env, &client, &owner);
}

#[test]
fn touch_ngo_bumps_instance_and_ngo_ttl() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    age_past_thresholds(&env, &client, &owner);

    client.touch_ngo(&owner);

    assert_ttls_bumped(&env, &client, &owner);
}
