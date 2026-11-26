// SPDX-License-Identifier: Apache-2.0

use super::*;
use soroban_sdk::testutils::{AuthorizedFunction, Events as _, MockAuth, MockAuthInvoke};
use soroban_sdk::{symbol_short, vec, Bytes, BytesN, IntoVal, Symbol};

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

// =============================================================================
// upgrade (contract bytecode upgrade)
// =============================================================================

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

// =============================================================================
// remove_ngo (issue #282)
// =============================================================================

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

// =============================================================================
// approver role management (issue #349)
// =============================================================================

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

    assert!(client.is_approver(&reviewer));
    assert_eq!(client.list_approvers(), vec![&env, reviewer.clone()]);
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

    assert!(!client.is_approver(&reviewer));
    assert_eq!(client.list_approvers().len(), 0);
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

// =============================================================================
// roles documentation parity
// =============================================================================

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
