// SPDX-License-Identifier: Apache-2.0
//! Shared fixtures and assertions for the registry tests.

use crate::*;
use common::DAY_IN_LEDGERS;
use soroban_sdk::testutils::storage::{Instance as _, Persistent as _};
use soroban_sdk::testutils::{Address as _, AuthorizedFunction, Events as _, Ledger};
use soroban_sdk::{vec, IntoVal, Symbol, Val, Vec};

pub fn setup() -> (Env, NgoRegistryClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    // Leave room for the 90-day NGO bump so the TTL tests see it unclamped.
    env.ledger()
        .with_mut(|l| l.max_entry_ttl = 365 * DAY_IN_LEDGERS);

    let admin = Address::generate(&env);
    let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    let client = NgoRegistryClient::new(&env, &contract_id);

    (env, client, admin)
}

/// A distinct NGO name for the `i`th of a batch of registrations ("NGO 000",
/// "NGO 001", ...): names must be unique, so a loop can't reuse one.
pub fn numbered_name(env: &Env, i: u32) -> String {
    let mut buf = *b"NGO 000";
    buf[4] = b'0' + (i / 100 % 10) as u8;
    buf[5] = b'0' + (i / 10 % 10) as u8;
    buf[6] = b'0' + (i % 10) as u8;
    String::from_bytes(env, &buf)
}

/// Asserts that the last call published exactly one event, from the
/// registry, with `topics` and `data`.
pub fn assert_only_event<T, D>(env: &Env, client: &NgoRegistryClient, topics: T, data: D)
where
    T: IntoVal<Env, Vec<Val>>,
    D: IntoVal<Env, Val>,
{
    let topics: Vec<Val> = topics.into_val(env);
    let data: Val = data.into_val(env);
    assert_eq!(
        env.events().all(),
        vec![env, (client.address.clone(), topics, data)]
    );
}

pub fn instance_ttl(env: &Env, client: &NgoRegistryClient) -> u32 {
    env.as_contract(&client.address, || env.storage().instance().get_ttl())
}

pub fn ngo_ttl(env: &Env, client: &NgoRegistryClient, owner: &Address) -> u32 {
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
pub fn age_past_thresholds(env: &Env, client: &NgoRegistryClient, owner: &Address) {
    env.ledger()
        .with_mut(|l| l.sequence_number += 2 * DAY_IN_LEDGERS);
    assert!(instance_ttl(env, client) < INSTANCE_LIFETIME_THRESHOLD);
    assert!(ngo_ttl(env, client, owner) < NGO_LIFETIME_THRESHOLD);
}

pub fn assert_ttls_bumped(env: &Env, client: &NgoRegistryClient, owner: &Address) {
    assert_eq!(instance_ttl(env, client), INSTANCE_BUMP_AMOUNT);
    assert_eq!(ngo_ttl(env, client, owner), NGO_BUMP_AMOUNT);
}

/// Asserts that the last call recorded exactly one authorization: `expected`
/// authorizing `function` on the registry itself.
pub fn assert_sole_auth(env: &Env, client: &NgoRegistryClient, expected: &Address, function: &str) {
    let auths = env.auths();
    assert_eq!(auths.len(), 1, "expected exactly one authorizer");
    let (address, invocation) = &auths[0];
    assert_eq!(address, expected);
    match &invocation.function {
        AuthorizedFunction::Contract((contract, name, _)) => {
            assert_eq!(contract, &client.address);
            assert_eq!(name, &Symbol::new(env, function));
        }
        _ => panic!("expected a contract invocation"),
    }
}

pub fn name_of(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}
