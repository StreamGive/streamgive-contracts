// SPDX-License-Identifier: Apache-2.0
//! Shared test fixtures and helper utilities used across all feature modules.

use soroban_sdk::testutils::{Address as _, AuthorizedFunction, Events as _, Ledger};
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::xdr::{ContractEventBody, ScVal, ScVec};
use soroban_sdk::{symbol_short, Env, IntoVal, Symbol, Val};

use crate::*;

// ── Event helpers ─────────────────────────────────────────────────────────────

/// Asserts that the `n`-th most recently published event (0 = the last one)
/// has the given topics and data, regardless of which contract emitted it.
pub fn assert_nth_last_event<T: IntoVal<Env, Val>, D: IntoVal<Env, Val>>(
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

/// Asserts that the most recently published event has the given topics and data.
pub fn assert_last_event<T: IntoVal<Env, Val>, D: IntoVal<Env, Val>>(
    env: &Env,
    topics: T,
    data: D,
) {
    assert_nth_last_event(env, 0, topics, data);
}

/// Counts the published events whose first topic equals `topic0`.
pub fn count_events_with_topic(env: &Env, topic0: Symbol) -> u32 {
    let all = env.events().all();
    let expected = ScVal::try_from_val(env, &topic0.into_val(env)).unwrap();
    all.events()
        .iter()
        .filter(|event| {
            let ContractEventBody::V0(body) = &event.body;
            body.topics.first() == Some(&expected)
        })
        .count() as u32
}

/// The most recently published event, in XDR form.
pub struct LastEvent {
    pub env: Env,
    pub topics: ScVal,
    pub data: ScVal,
}

impl core::fmt::Debug for LastEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "({:?}, {:?})", self.topics, self.data)
    }
}

impl PartialEq<(soroban_sdk::Vec<Val>, Val)> for LastEvent {
    fn eq(&self, (topics, data): &(soroban_sdk::Vec<Val>, Val)) -> bool {
        soroban_sdk::xdr::ScVal::try_from_val(&self.env, &topics.to_val()).unwrap() == self.topics
            && soroban_sdk::xdr::ScVal::try_from_val(&self.env, data).unwrap() == self.data
    }
}

/// Returns the topics and data of the most recently published event.
pub fn last_event(env: &Env) -> LastEvent {
    let all = env.events().all();
    let ContractEventBody::V0(body) = &all.events().last().unwrap().body;
    LastEvent {
        env: env.clone(),
        topics: ScVal::Vec(Some(ScVec(body.topics.clone()))),
        data: body.data.clone(),
    }
}

// ── Token factory ─────────────────────────────────────────────────────────────

pub fn create_token<'a>(env: &Env, admin: &Address) -> (TokenClient<'a>, StellarAssetClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    (
        TokenClient::new(env, &sac.address()),
        StellarAssetClient::new(env, &sac.address()),
    )
}

// ── Main test fixture ─────────────────────────────────────────────────────────

pub struct Setup<'a> {
    pub env: Env,
    pub client: DonationVaultClient<'a>,
    pub token: TokenClient<'a>,
    pub token_admin: StellarAssetClient<'a>,
    pub donor: Address,
    pub ngo: Address,
}

pub fn setup() -> Setup<'static> {
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

// ── Registry fixture ──────────────────────────────────────────────────────────

pub fn setup_with_registry() -> (Setup<'static>, ngo_registry::NgoRegistryClient<'static>) {
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

// ── Auth assertion ────────────────────────────────────────────────────────────

/// Asserts that the last top-level call required auth from exactly one address,
/// `expected`, for `fn_name` on the vault.
pub fn assert_auth_required_from(s: &Setup, expected: &Address, fn_name: &str) {
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

// ── TTL helpers ───────────────────────────────────────────────────────────────

pub fn instance_ttl(s: &Setup) -> u32 {
    s.env
        .as_contract(&s.client.address, || s.env.storage().instance().get_ttl())
}

pub fn stream_ttl(s: &Setup, stream_id: u64) -> u32 {
    s.env.as_contract(&s.client.address, || {
        s.env
            .storage()
            .persistent()
            .get_ttl(&DataKey::Stream(stream_id))
    })
}

/// Moves the ledger forward two days, dropping both instance and stream below
/// their lifetime thresholds.
pub fn age_past_thresholds(s: &Setup, stream_id: Option<u64>) {
    s.env
        .ledger()
        .with_mut(|l| l.sequence_number += 2 * DAY_IN_LEDGERS);
    assert!(instance_ttl(s) < INSTANCE_LIFETIME_THRESHOLD);
    if let Some(stream_id) = stream_id {
        assert!(stream_ttl(s, stream_id) < STREAM_LIFETIME_THRESHOLD);
    }
}

pub fn create_ttl_test_stream(s: &Setup) -> u64 {
    s.token_admin.mint(&s.donor, &2_000);
    s.client
        .create_stream(&s.donor, &s.ngo, &s.token.address, &1_000, &10)
}

// ── Storage manipulation ──────────────────────────────────────────────────────

/// Rewrites a stored stream in place, for pushing its bookkeeping to the
/// edge of i128.
pub fn overwrite_stream(s: &Setup, stream_id: u64, edit: impl FnOnce(&mut Stream)) {
    s.env.as_contract(&s.client.address, || {
        let key = DataKey::Stream(stream_id);
        let mut stream: Stream = s.env.storage().persistent().get(&key).unwrap();
        edit(&mut stream);
        s.env.storage().persistent().set(&key, &stream);
    });
}
