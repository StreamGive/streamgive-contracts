// SPDX-License-Identifier: Apache-2.0
//! Tests for registering, renaming, describing and leaving the registry:
//! `register`, `unregister`, `update_name`, name validation and uniqueness,
//! `owner_by_name`, metadata URIs, categories, `touch_ngo` and owner transfer.

use super::helpers::*;
use crate::*;
use soroban_sdk::testutils::storage::Persistent as _;
use soroban_sdk::testutils::{
    Address as _, AuthorizedFunction, Events as _, MockAuth, MockAuthInvoke,
};
use soroban_sdk::{symbol_short, vec, IntoVal, Symbol, Val, Vec};

// ── register and unregister ───────────────────────────────────────────────────

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
fn unverified_ngo_can_unregister_and_register_again() {
    let (env, client, _) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Pending NGO");

    client.register(&owner, &name);
    client.unregister(&owner);
    assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));

    client.register(&owner, &String::from_str(&env, "Updated NGO"));
    assert_eq!(
        client.get_ngo(&owner).name,
        String::from_str(&env, "Updated NGO")
    );
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
fn register_bumps_instance_and_ngo_ttl() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    assert_ttls_bumped(&env, &client, &owner);
}

#[test]
fn register_publishes_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");

    client.register(&owner, &name);

    assert_eq!(
        env.events().all(),
        soroban_sdk::vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("register"), owner).into_val(&env),
                name.into_val(&env),
            ),
        ]
    );
}

#[test]
fn register_requires_owner_auth() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);

    client.register(&owner, &String::from_str(&env, "Red Cross"));

    assert_sole_auth(&env, &client, &owner, "register");
}

#[test]
#[should_panic]
fn register_without_owner_auth_fails() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);

    env.mock_auths(&[]);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
}

#[test]
fn unregister_requires_owner_auth() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.unregister(&owner);

    assert_sole_auth(&env, &client, &owner, "unregister");
}

#[test]
#[should_panic]
fn unregister_without_owner_auth_fails() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    env.mock_auths(&[]);
    client.unregister(&owner);
}

#[test]
fn unregister_publishes_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.unregister(&owner);

    // Events are cleared per top-level call; only the unregist event is visible.
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("unregist"), owner).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );
}

#[test]
fn unregister_verified_ngo_fails() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    let result = client.try_unregister(&owner);

    assert_eq!(result, Err(Ok(Error::AlreadyVerified)));
    // The entry survives the rejected call, verification intact.
    assert!(client.get_ngo(&owner).verified);
    assert_eq!(client.list_ngos(&0, &10).len(), 1);
}

// ── Names and name uniqueness ─────────────────────────────────────────────────

#[test]
fn register_whitespace_only_name_fails() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let whitespace = String::from_str(&env, " \t\n\r");

    let result = client.try_register(&owner, &whitespace);

    assert_eq!(result, Err(Ok(Error::InvalidName)));
    assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
}

#[test]
fn register_rejects_name_over_max_length() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    // MAX_NGO_NAME_LEN (200) + 1 bytes.
    let too_long = String::from_str(&env, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

    let result = client.try_register(&owner, &too_long);

    assert_eq!(result, Err(Ok(Error::NameTooLong)));
    assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
}

#[test]
fn register_accepts_name_at_max_length() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    // Exactly MAX_NGO_NAME_LEN (200) bytes.
    let at_limit = String::from_str(&env, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

    client.register(&owner, &at_limit);

    assert_eq!(client.get_ngo(&owner).name, at_limit);
}

#[test]
fn update_name_rejects_name_over_max_length() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    let too_long = String::from_str(&env, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

    let result = client.try_update_name(&owner, &too_long);

    assert_eq!(result, Err(Ok(Error::NameTooLong)));
}

#[test]
fn update_name_accepts_name_at_max_length() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Initial NGO"));
    // Exactly MAX_NGO_NAME_LEN (200) bytes.
    let at_limit = String::from_str(&env, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

    client.update_name(&owner, &at_limit);
    assert_eq!(client.get_ngo(&owner).name, at_limit);
}

#[test]
fn register_and_update_name_unicode_whitespace_only_fails() {
    let (env, client, _admin) = setup();
    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    client.register(&owner2, &String::from_str(&env, "Valid NGO"));

    let test_cases = [
        "\u{00a0}",                         // Non-breaking space
        "\u{3000}",                         // Ideographic space
        "\u{2003}",                         // Em space
        " \t\u{00a0}\u{3000}\u{2003}\n\r ", // Mix with ASCII whitespace
    ];

    for case in test_cases.iter() {
        let ws = String::from_str(&env, case);
        let res_reg = client.try_register(&owner1, &ws);
        assert_eq!(res_reg, Err(Ok(Error::InvalidName)));

        let res_upd = client.try_update_name(&owner2, &ws);
        assert_eq!(res_upd, Err(Ok(Error::InvalidName)));
    }
}

#[test]
fn register_and_update_name_zero_width_invisible_only_fails() {
    let (env, client, _admin) = setup();
    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    client.register(&owner2, &String::from_str(&env, "Valid NGO"));

    let test_cases = [
        "\u{200b}",                                 // Zero-width space
        "\u{200c}",                                 // Zero-width non-joiner
        "\u{200d}",                                 // Zero-width joiner
        "\u{2060}",                                 // Word joiner
        "\u{feff}",                                 // BOM / Zero-width no-break space
        "\u{200b}\u{200c}\u{200d}\u{2060}\u{feff}", // All invisible
        " \u{200b} \t\u{feff}\n",                   // Mixed with whitespace
    ];

    for case in test_cases.iter() {
        let inv = String::from_str(&env, case);
        let res_reg = client.try_register(&owner1, &inv);
        assert_eq!(res_reg, Err(Ok(Error::InvalidName)));

        let res_upd = client.try_update_name(&owner2, &inv);
        assert_eq!(res_upd, Err(Ok(Error::InvalidName)));
    }
}

#[test]
fn register_and_update_name_control_characters_fail() {
    let (env, client, _admin) = setup();
    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    client.register(&owner2, &String::from_str(&env, "Valid NGO"));

    let test_cases = [
        "Charity\u{0000}Name", // NUL
        "Charity\u{0007}Name", // BEL
        "Charity\u{001b}Name", // ESC
        "Charity\u{007f}Name", // DEL
        "Charity\u{0080}Name", // C1 control (U+0080)
        "Charity\u{009f}Name", // C1 control (U+009F)
    ];

    for case in test_cases.iter() {
        let ctrl = String::from_str(&env, case);
        let res_reg = client.try_register(&owner1, &ctrl);
        assert_eq!(res_reg, Err(Ok(Error::InvalidName)));

        let res_upd = client.try_update_name(&owner2, &ctrl);
        assert_eq!(res_upd, Err(Ok(Error::InvalidName)));
    }
}

#[test]
fn register_and_update_name_internal_unicode_space_succeeds() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name_ideographic = String::from_str(&env, "Red\u{3000}Cross");
    let name_nbsp = String::from_str(&env, "Red\u{00a0}Cross");

    // Register with internal ideographic space succeeds
    client.register(&owner, &name_ideographic);
    assert_eq!(client.get_ngo(&owner).name, name_ideographic);

    // Update with internal non-breaking space succeeds
    client.update_name(&owner, &name_nbsp);
    assert_eq!(client.get_ngo(&owner).name, name_nbsp);
}

#[test]
fn register_and_update_name_byte_boundaries_and_multibyte() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);

    // Greek capital Omega 'Ω' (U+03A9) is 2 bytes in UTF-8 (0xCE 0xA9).
    // 100 * 2 bytes = 200 bytes, exactly MAX_NGO_NAME_LEN.
    let omega_100_str = "ΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩ";
    assert_eq!(omega_100_str.len(), 200);
    let omega_100 = String::from_str(&env, omega_100_str);

    client.register(&owner, &omega_100);
    assert_eq!(client.get_ngo(&owner).name, omega_100);

    // 101 Greek capital Omegas = 202 bytes > 200 bytes, rejected with NameTooLong.
    let omega_101_str = "ΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩΩ";
    assert_eq!(omega_101_str.len(), 202);
    let omega_101 = String::from_str(&env, omega_101_str);

    let res_upd = client.try_update_name(&owner, &omega_101);
    assert_eq!(res_upd, Err(Ok(Error::NameTooLong)));

    let owner2 = Address::generate(&env);
    let res_reg = client.try_register(&owner2, &omega_101);
    assert_eq!(res_reg, Err(Ok(Error::NameTooLong)));

    // Multi-byte at boundary: 198 'a's + 1 'Ω' (2 bytes) = 200 bytes -> accepted
    let boundary_200_str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaΩ";
    assert_eq!(boundary_200_str.len(), 200);
    let boundary_200 = String::from_str(&env, boundary_200_str);
    client.update_name(&owner, &boundary_200);
    assert_eq!(client.get_ngo(&owner).name, boundary_200);

    // 199 'a's + 1 'Ω' (2 bytes) = 201 bytes -> rejected with NameTooLong without panicking
    let boundary_201_str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaΩ";
    assert_eq!(boundary_201_str.len(), 201);
    let boundary_201 = String::from_str(&env, boundary_201_str);
    let res_boundary = client.try_update_name(&owner, &boundary_201);
    assert_eq!(res_boundary, Err(Ok(Error::NameTooLong)));
}

#[test]
fn validate_name_bytes_rejects_invalid_utf8() {
    // 0xFF and 0xFE are never valid in UTF-8
    assert_eq!(validate_name_bytes(&[0xFF, 0xFE]), Err(Error::InvalidName));
    // Overlong NUL (0xC0 0x80)
    assert_eq!(validate_name_bytes(&[0xC0, 0x80]), Err(Error::InvalidName));
    // Lone continuation byte (0x80)
    assert_eq!(validate_name_bytes(&[0x80]), Err(Error::InvalidName));
}

#[test]
fn update_name_changes_name_before_approval() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Crsos"));

    let fixed = String::from_str(&env, "Red Cross");
    client.update_name(&owner, &fixed);

    // Events cover only the latest top-level call, so read them before
    // `get_ngo` below replaces them.
    let events = env.events().all();
    let expected: Vec<(Address, Vec<Val>, Val)> = vec![
        &env,
        (
            client.address.clone(),
            (symbol_short!("renamed"), owner.clone()).into_val(&env),
            fixed.into_val(&env),
        ),
    ];
    assert_eq!(events, expected);

    assert_eq!(
        env.events().all(),
        soroban_sdk::vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("renamed"), owner.clone()).into_val(&env),
                fixed.into_val(&env)
            ),
        ]
    );

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("renamed"), owner.clone()).into_val(&env),
                fixed.into_val(&env),
            ),
        ]
    );

    assert_eq!(
        client.get_ngo(&owner),
        Ngo {
            owner: owner.clone(),
            name: fixed.clone(),
            verified: false,
            metadata_uri: None,
            verified_until: None,
            categories: 0,
            pending_owner: None,
        }
    );
}

#[test]
fn update_name_after_approval_fails() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");
    client.register(&owner, &name);
    client.approve_ngo(&admin, &owner);

    let result = client.try_update_name(&owner, &String::from_str(&env, "Blue Cross"));
    assert_eq!(result, Err(Ok(Error::AlreadyVerified)));
    assert_eq!(client.get_ngo(&owner).name, name);
}

#[test]
fn update_name_after_revocation_succeeds() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Crsos"));
    client.approve_ngo(&admin, &owner);

    // The name an admin approved is locked only while the NGO stays
    // verified, so the rename has to be rejected at this point.
    let blocked = client.try_update_name(&owner, &String::from_str(&env, "Red Cross"));
    assert_eq!(blocked, Err(Ok(Error::AlreadyVerified)));

    client.revoke_ngo(&admin, &owner);
    assert!(!client.get_ngo(&owner).verified);

    let fixed = String::from_str(&env, "Red Cross");
    client.update_name(&owner, &fixed);

    // Events cover only the latest top-level call, so read them before
    // `get_ngo` below replaces them.
    assert_eq!(
        env.events().all(),
        soroban_sdk::vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("renamed"), owner.clone()).into_val(&env),
                fixed.clone().into_val(&env),
            ),
        ]
    );

    assert_eq!(
        client.get_ngo(&owner),
        Ngo {
            owner: owner.clone(),
            name: fixed,
            verified: false,
            metadata_uri: None,
            verified_until: None,
            categories: 0,
            pending_owner: None,
        }
    );
}

#[test]
fn update_name_for_unregistered_ngo_fails() {
    let (env, client, _admin) = setup();
    let random = Address::generate(&env);

    let result = client.try_update_name(&random, &String::from_str(&env, "Red Cross"));
    assert_eq!(result, Err(Ok(Error::NotRegistered)));
}

#[test]
fn update_name_requires_owner_auth() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Crsos"));

    client.update_name(&owner, &String::from_str(&env, "Red Cross"));

    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    let (address, invocation) = &auths[0];
    assert_eq!(address, &owner);
    match &invocation.function {
        AuthorizedFunction::Contract((contract, function, _)) => {
            assert_eq!(contract, &client.address);
            assert_eq!(function, &Symbol::new(&env, "update_name"));
        }
        _ => panic!("expected a contract invocation"),
    }
}

#[test]
fn update_name_bumps_instance_and_ngo_ttl() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Crsos"));
    age_past_thresholds(&env, &client, &owner);

    client.update_name(&owner, &String::from_str(&env, "Red Cross"));

    assert_ttls_bumped(&env, &client, &owner);
}

#[test]
fn register_duplicate_name_exact_match_fails() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    client.register(&a, &String::from_str(&env, "Red Cross"));

    // A second registrant with the identical name must be rejected.
    let result = client.try_register(&b, &String::from_str(&env, "Red Cross"));
    assert_eq!(result, Err(Ok(Error::NameTaken)));
    assert_eq!(client.try_get_ngo(&b), Err(Ok(Error::NotRegistered)));
}

#[test]
fn register_duplicate_name_case_folded_fails() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    client.register(&a, &String::from_str(&env, "Red Cross"));

    // All-uppercase canonical form matches the existing name.
    assert_eq!(
        client.try_register(&b, &String::from_str(&env, "RED CROSS")),
        Err(Ok(Error::NameTaken))
    );
    // Mixed case also matches.
    assert_eq!(
        client.try_register(&c, &String::from_str(&env, "rEd cRoSs")),
        Err(Ok(Error::NameTaken))
    );
    // Neither registrant was stored.
    assert_eq!(client.try_get_ngo(&b), Err(Ok(Error::NotRegistered)));
    assert_eq!(client.try_get_ngo(&c), Err(Ok(Error::NotRegistered)));
}

#[test]
fn register_duplicate_name_trimmed_whitespace_fails() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    client.register(&a, &String::from_str(&env, "Red Cross"));

    // Leading/trailing ASCII spaces collapse to the same canonical key.
    let result = client.try_register(&b, &String::from_str(&env, "  Red Cross  "));
    assert_eq!(result, Err(Ok(Error::NameTaken)));
    assert_eq!(client.try_get_ngo(&b), Err(Ok(Error::NotRegistered)));
}

#[test]
fn register_different_names_both_succeed() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    client.register(&a, &String::from_str(&env, "Red Cross"));
    client.register(&b, &String::from_str(&env, "Oxfam"));

    assert_eq!(client.get_ngo(&a).name, String::from_str(&env, "Red Cross"));
    assert_eq!(client.get_ngo(&b).name, String::from_str(&env, "Oxfam"));
    assert_eq!(client.total_ngos(), 2);
}

#[test]
fn update_name_to_taken_name_fails() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    client.register(&a, &String::from_str(&env, "Red Cross"));
    client.register(&b, &String::from_str(&env, "Oxfam"));

    // B tries to rename itself to A's existing name.
    let result = client.try_update_name(&b, &String::from_str(&env, "Red Cross"));
    assert_eq!(result, Err(Ok(Error::NameTaken)));

    // B's name is unchanged.
    assert_eq!(client.get_ngo(&b).name, String::from_str(&env, "Oxfam"));
}

#[test]
fn update_name_to_taken_name_case_folded_fails() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    client.register(&a, &String::from_str(&env, "Red Cross"));
    client.register(&b, &String::from_str(&env, "Oxfam"));

    assert_eq!(
        client.try_update_name(&b, &String::from_str(&env, "RED CROSS")),
        Err(Ok(Error::NameTaken))
    );
    assert_eq!(client.get_ngo(&b).name, String::from_str(&env, "Oxfam"));
}

#[test]
fn update_name_to_own_current_name_is_noop_no_error() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);

    client.register(&owner, &String::from_str(&env, "Red Cross"));

    // Updating to the same name does not trigger NameTaken — it short-circuits
    // before the uniqueness check because the name hasn't changed.
    client.update_name(&owner, &String::from_str(&env, "Red Cross"));
    assert_eq!(
        client.get_ngo(&owner).name,
        String::from_str(&env, "Red Cross")
    );
}

#[test]
fn unregister_frees_name_so_another_can_claim_it() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    client.register(&a, &String::from_str(&env, "Red Cross"));
    client.unregister(&a);

    // The name is now free — a different address should be able to register it.
    client.register(&b, &String::from_str(&env, "Red Cross"));
    assert_eq!(client.get_ngo(&b).name, String::from_str(&env, "Red Cross"));
}

#[test]
fn remove_ngo_frees_name_so_another_can_claim_it() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    client.register(&a, &String::from_str(&env, "Red Cross"));
    client.remove_ngo(&a);

    client.register(&b, &String::from_str(&env, "Red Cross"));
    assert_eq!(client.get_ngo(&b).name, String::from_str(&env, "Red Cross"));
}

#[test]
fn reject_ngo_frees_name_so_another_can_claim_it() {
    let (env, client, admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    client.register(&a, &String::from_str(&env, "Red Cross"));
    client.reject_ngo(&admin, &a, &1);

    client.register(&b, &String::from_str(&env, "Red Cross"));
    assert_eq!(client.get_ngo(&b).name, String::from_str(&env, "Red Cross"));
}

#[test]
fn update_name_frees_old_name_so_another_can_claim_it() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    client.register(&a, &String::from_str(&env, "Red Cross"));
    client.update_name(&a, &String::from_str(&env, "Red Cross UK"));

    // "Red Cross" is now free — b can take it.
    client.register(&b, &String::from_str(&env, "Red Cross"));
    assert_eq!(client.get_ngo(&b).name, String::from_str(&env, "Red Cross"));
    // a still holds "Red Cross UK".
    assert_eq!(
        client.get_ngo(&a).name,
        String::from_str(&env, "Red Cross UK")
    );
}

#[test]
fn name_uniqueness_is_enforced_across_case_and_whitespace_together() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    // Register with leading space and mixed case.
    client.register(&a, &String::from_str(&env, "  Red Cross  "));

    // Exact trimmed lowercase match must still be blocked.
    let result = client.try_register(&b, &String::from_str(&env, "red cross"));
    assert_eq!(result, Err(Ok(Error::NameTaken)));
}

// ── owner_by_name ─────────────────────────────────────────────────────────────

#[test]
fn owner_by_name_is_none_for_an_unknown_name() {
    let (env, client, _admin) = setup();
    assert_eq!(client.owner_by_name(&name_of(&env, "Nobody")), None);
}

#[test]
fn owner_by_name_finds_the_owner_after_register() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let other = Address::generate(&env);
    client.register(&owner, &name_of(&env, "Red Cross"));
    client.register(&other, &name_of(&env, "Green Fund"));

    assert_eq!(
        client.owner_by_name(&name_of(&env, "Red Cross")),
        Some(owner)
    );
    assert_eq!(
        client.owner_by_name(&name_of(&env, "Green Fund")),
        Some(other)
    );
    // Exact match only.
    assert_eq!(client.owner_by_name(&name_of(&env, "red cross")), None);
}

#[test]
fn owner_by_name_follows_a_rename() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &name_of(&env, "Exmaple NGO"));

    client.update_name(&owner, &name_of(&env, "Example NGO"));

    assert_eq!(client.owner_by_name(&name_of(&env, "Exmaple NGO")), None);
    assert_eq!(
        client.owner_by_name(&name_of(&env, "Example NGO")),
        Some(owner)
    );
}

#[test]
fn owner_by_name_ignores_a_rename_to_the_same_name() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &name_of(&env, "Red Cross"));

    client.update_name(&owner, &name_of(&env, "Red Cross"));

    assert_eq!(
        client.owner_by_name(&name_of(&env, "Red Cross")),
        Some(owner)
    );
}

#[test]
fn owner_by_name_is_cleared_by_unregister() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &name_of(&env, "Red Cross"));

    client.unregister(&owner);

    assert_eq!(client.owner_by_name(&name_of(&env, "Red Cross")), None);
}

#[test]
fn owner_by_name_is_cleared_by_remove_ngo() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &name_of(&env, "Red Cross"));

    client.remove_ngo(&owner);

    assert_eq!(client.owner_by_name(&name_of(&env, "Red Cross")), None);
}

#[test]
fn owner_by_name_is_cleared_by_reject_ngo() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &name_of(&env, "Red Cross"));

    client.reject_ngo(&admin, &owner, &1);

    assert_eq!(client.owner_by_name(&name_of(&env, "Red Cross")), None);
}

#[test]
fn owner_by_name_survives_approval_and_revocation() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &name_of(&env, "Red Cross"));

    client.approve_ngo(&admin, &owner);
    assert_eq!(
        client.owner_by_name(&name_of(&env, "Red Cross")),
        Some(owner.clone())
    );

    client.revoke_ngo(&admin, &owner);
    assert_eq!(
        client.owner_by_name(&name_of(&env, "Red Cross")),
        Some(owner)
    );
}

#[test]
fn owner_by_name_lets_a_name_be_reused_after_the_owner_leaves() {
    let (env, client, _admin) = setup();
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    client.register(&first, &name_of(&env, "Red Cross"));
    client.unregister(&first);

    client.register(&second, &name_of(&env, "Red Cross"));

    assert_eq!(
        client.owner_by_name(&name_of(&env, "Red Cross")),
        Some(second)
    );
}

#[test]
fn owner_by_name_index_survives_a_swap_and_pop_removal() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    client.register(&a, &name_of(&env, "A"));
    client.register(&b, &name_of(&env, "B"));
    client.register(&c, &name_of(&env, "C"));

    client.remove_ngo(&a);

    assert_eq!(client.owner_by_name(&name_of(&env, "A")), None);
    assert_eq!(client.owner_by_name(&name_of(&env, "B")), Some(b));
    assert_eq!(client.owner_by_name(&name_of(&env, "C")), Some(c));
}

#[test]
fn touch_ngo_refreshes_the_name_index_ttl() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name = name_of(&env, "Red Cross");
    client.register(&owner, &name);
    age_past_thresholds(&env, &client, &owner);

    client.touch_ngo(&owner);

    let ttl = env.as_contract(&client.address, || {
        env.storage().persistent().get_ttl(&name_key(&env, &name))
    });
    assert_eq!(ttl, NGO_BUMP_AMOUNT);
}

#[test]
fn owner_by_name_works_for_a_max_length_name() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let long = String::from_bytes(&env, &[b'a'; MAX_NGO_NAME_LEN as usize]);
    client.register(&owner, &long);

    assert_eq!(client.owner_by_name(&long), Some(owner));
}

// ── touch_ngo ─────────────────────────────────────────────────────────────────

#[test]
fn touch_ngo_leaves_entry_unchanged() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");
    client.register(&owner, &name);
    client.approve_ngo(&admin, &owner);

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

#[test]
fn touch_ngo_bumps_instance_and_ngo_ttl() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    age_past_thresholds(&env, &client, &owner);

    client.touch_ngo(&owner);

    assert_ttls_bumped(&env, &client, &owner);
}

// ── Metadata URI ──────────────────────────────────────────────────────────────

#[test]
fn register_defaults_metadata_uri_to_none() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    assert_eq!(client.get_ngo(&owner).metadata_uri, None);
    assert_eq!(client.metadata_uri(&owner), None);
}

#[test]
fn metadata_uri_getter_returns_none_for_unregistered_address() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);

    assert_eq!(client.metadata_uri(&stranger), None);
}

#[test]
fn set_metadata_uri_stores_uri_and_publishes_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    let uri = Some(String::from_str(&env, "https://example.org/about"));
    client.set_metadata_uri(&owner, &uri);

    // Events cover only the latest top-level call, so assert before get_ngo.
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("metadata"), owner.clone()).into_val(&env),
                uri.clone().into_val(&env),
            ),
        ]
    );

    assert_eq!(client.get_ngo(&owner).metadata_uri, uri);
    assert_eq!(client.metadata_uri(&owner), uri);
}

#[test]
fn set_metadata_uri_clears_with_none() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.set_metadata_uri(&owner, &Some(String::from_str(&env, "https://example.org")));

    client.set_metadata_uri(&owner, &None);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("metadata"), owner.clone()).into_val(&env),
                None::<String>.into_val(&env),
            ),
        ]
    );
    assert_eq!(client.get_ngo(&owner).metadata_uri, None);
    assert_eq!(client.metadata_uri(&owner), None);
}

#[test]
fn set_metadata_uri_no_event_when_unchanged() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    let uri = Some(String::from_str(&env, "https://example.org"));
    client.set_metadata_uri(&owner, &uri);

    // Re-setting the same value writes nothing and emits nothing.
    client.set_metadata_uri(&owner, &uri);

    assert_eq!(env.events().all().events().len(), 0);
}

#[test]
fn set_metadata_uri_accepts_uri_at_max_length() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    // Exactly MAX_METADATA_URI_LEN (512) bytes.
    let at_limit = String::from_bytes(&env, &[b'a'; MAX_METADATA_URI_LEN as usize]);
    client.set_metadata_uri(&owner, &Some(at_limit.clone()));

    assert_eq!(client.get_ngo(&owner).metadata_uri, Some(at_limit));
}

#[test]
fn set_metadata_uri_rejects_uri_over_max_length() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    // MAX_METADATA_URI_LEN (512) + 1 bytes.
    let too_long = String::from_bytes(&env, &[b'a'; MAX_METADATA_URI_LEN as usize + 1]);
    let result = client.try_set_metadata_uri(&owner, &Some(too_long));

    assert_eq!(result, Err(Ok(Error::MetadataUriTooLong)));
    // The entry is left untouched.
    assert_eq!(client.get_ngo(&owner).metadata_uri, None);
}

#[test]
fn set_metadata_uri_for_unregistered_ngo_fails() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);

    let result = client.try_set_metadata_uri(
        &stranger,
        &Some(String::from_str(&env, "https://example.org")),
    );

    assert_eq!(result, Err(Ok(Error::NotRegistered)));
}

#[test]
fn set_metadata_uri_requires_owner_auth() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.set_metadata_uri(&owner, &Some(String::from_str(&env, "https://example.org")));

    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    let (address, invocation) = &auths[0];
    assert_eq!(address, &owner);
    match &invocation.function {
        AuthorizedFunction::Contract((contract, function, _)) => {
            assert_eq!(contract, &client.address);
            assert_eq!(function, &Symbol::new(&env, "set_metadata_uri"));
        }
        _ => panic!("expected a contract invocation"),
    }
}

#[test]
fn set_metadata_uri_remains_allowed_after_approval() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    // Unlike the name, the metadata URI is not frozen by verification.
    let uri = Some(String::from_str(&env, "https://example.org/logo.svg"));
    client.set_metadata_uri(&owner, &uri);

    let ngo = client.get_ngo(&owner);
    assert!(ngo.verified);
    assert_eq!(ngo.metadata_uri, uri);
}

#[test]
fn set_metadata_uri_bumps_instance_and_ngo_ttl() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    age_past_thresholds(&env, &client, &owner);

    client.set_metadata_uri(&owner, &Some(String::from_str(&env, "https://example.org")));

    assert_ttls_bumped(&env, &client, &owner);
}

#[test]
fn get_ngo_includes_metadata_uri() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");
    client.register(&owner, &name);
    let uri = Some(String::from_str(&env, "https://example.org"));
    client.set_metadata_uri(&owner, &uri);

    assert_eq!(
        client.get_ngo(&owner),
        Ngo {
            owner: owner.clone(),
            name,
            verified: false,
            metadata_uri: uri,
            verified_until: None,
            categories: 0,
            pending_owner: None,
        }
    );
}

// ── Categories ────────────────────────────────────────────────────────────────

#[test]
fn categories_defaults_to_zero_after_registration() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    assert_eq!(client.categories(&owner), 0);
    assert_eq!(client.get_ngo(&owner).categories, 0);
}

#[test]
fn categories_returns_zero_for_unregistered_address() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);

    // Never panics or errors — just returns 0.
    assert_eq!(client.categories(&stranger), 0);
}

#[test]
fn set_categories_stores_value_and_emits_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    let mask = CATEGORY_HEALTH | CATEGORY_EDUCATION;
    client.set_categories(&owner, &mask);
    assert_only_event(
        &env,
        &client,
        (symbol_short!("catset"), owner.clone()),
        mask,
    );

    assert_eq!(client.categories(&owner), mask);
    assert_eq!(client.get_ngo(&owner).categories, mask);
}

#[test]
fn set_categories_to_zero_clears_all_tags() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.set_categories(&owner, &CATEGORY_ENVIRONMENT);
    assert_eq!(client.categories(&owner), CATEGORY_ENVIRONMENT);

    client.set_categories(&owner, &0);
    assert_eq!(client.categories(&owner), 0);
}

#[test]
fn set_categories_no_event_when_value_unchanged() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    let mask = CATEGORY_HUMANITAR | CATEGORY_ANIMALS;
    client.set_categories(&owner, &mask);

    // Setting the same mask again must write nothing and emit nothing.
    client.set_categories(&owner, &mask);

    assert_eq!(env.events().all().events().len(), 0);
    assert_eq!(client.categories(&owner), mask);
}

#[test]
fn set_categories_accepts_every_defined_bit() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.set_categories(&owner, &VALID_CATEGORY_MASK);
    assert_eq!(client.categories(&owner), VALID_CATEGORY_MASK);
}

#[test]
fn set_categories_rejects_reserved_bits() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    // Bit 8 is reserved.
    let with_reserved = CATEGORY_HEALTH | (1u32 << 8);
    assert_eq!(
        client.try_set_categories(&owner, &with_reserved),
        Err(Ok(Error::InvalidCategories))
    );
    // No change.
    assert_eq!(client.categories(&owner), 0);
}

#[test]
fn set_categories_rejects_high_reserved_bit() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    let high_bit = 1u32 << 31;
    assert_eq!(
        client.try_set_categories(&owner, &high_bit),
        Err(Ok(Error::InvalidCategories))
    );
    assert_eq!(client.categories(&owner), 0);
}

#[test]
fn set_categories_locked_after_verification() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    assert_eq!(
        client.try_set_categories(&owner, &CATEGORY_ARTS),
        Err(Ok(Error::AlreadyVerified))
    );
    // Categories stay at the pre-approval value (0).
    assert_eq!(client.categories(&owner), 0);
}

#[test]
fn set_categories_allowed_again_after_revocation() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    // Set before approval.
    client.set_categories(&owner, &CATEGORY_HEALTH);
    client.approve_ngo(&admin, &owner);

    // Locked — cannot change while verified.
    assert_eq!(
        client.try_set_categories(&owner, &CATEGORY_EDUCATION),
        Err(Ok(Error::AlreadyVerified))
    );

    // After revocation the owner can update categories again.
    client.revoke_ngo(&admin, &owner);
    client.set_categories(&owner, &CATEGORY_EDUCATION);
    assert_eq!(client.categories(&owner), CATEGORY_EDUCATION);
}

#[test]
fn set_categories_for_unregistered_ngo_fails() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);

    assert_eq!(
        client.try_set_categories(&stranger, &CATEGORY_COMMUNITY),
        Err(Ok(Error::NotRegistered))
    );
}

#[test]
fn set_categories_requires_owner_auth() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.set_categories(&owner, &CATEGORY_RESEARCH);

    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    let (address, invocation) = &auths[0];
    assert_eq!(address, &owner);
    match &invocation.function {
        AuthorizedFunction::Contract((contract, function, _)) => {
            assert_eq!(contract, &client.address);
            assert_eq!(function, &Symbol::new(&env, "set_categories"));
        }
        _ => panic!("expected a contract invocation"),
    }
}

#[test]
fn set_categories_bumps_instance_and_ngo_ttl() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    age_past_thresholds(&env, &client, &owner);

    client.set_categories(&owner, &CATEGORY_HEALTH);

    assert_ttls_bumped(&env, &client, &owner);
}

#[test]
fn get_ngo_includes_categories() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");
    client.register(&owner, &name);
    let mask = CATEGORY_HEALTH | CATEGORY_HUMANITAR;
    client.set_categories(&owner, &mask);

    assert_eq!(
        client.get_ngo(&owner),
        Ngo {
            owner: owner.clone(),
            name,
            verified: false,
            metadata_uri: None,
            verified_until: None,
            categories: mask,
            pending_owner: None,
        }
    );
}

#[test]
fn set_categories_can_be_updated_multiple_times_before_verification() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.set_categories(&owner, &CATEGORY_HEALTH);
    assert_eq!(client.categories(&owner), CATEGORY_HEALTH);

    client.set_categories(&owner, &(CATEGORY_HEALTH | CATEGORY_EDUCATION));
    assert_eq!(
        client.categories(&owner),
        CATEGORY_HEALTH | CATEGORY_EDUCATION
    );

    client.set_categories(&owner, &CATEGORY_ENVIRONMENT);
    assert_eq!(client.categories(&owner), CATEGORY_ENVIRONMENT);
}

// ── Owner transfer ────────────────────────────────────────────────────────────

#[test]
fn ngo_owner_transfer_preserves_verified_flag_on_propose() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    client.register(&old_owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &old_owner);

    client.propose_owner_transfer(&old_owner, &new_owner);

    // Until the transfer is accepted the entry stays with the old owner.
    assert!(client.is_verified(&old_owner));
    let ngo = client.get_ngo(&old_owner);
    assert!(ngo.verified);
    assert_eq!(ngo.owner, old_owner);
    assert_eq!(
        client.try_get_ngo(&new_owner),
        Err(Ok(Error::NotRegistered))
    );
}

#[test]
fn ngo_owner_transfer_preserves_verified_flag_on_accept() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    client.register(&old_owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &old_owner);

    client.propose_owner_transfer(&old_owner, &new_owner);
    client.accept_owner_transfer(&old_owner, &new_owner);

    let new_ngo = client.get_ngo(&new_owner);
    assert_eq!(new_ngo.owner, new_owner);
    assert!(
        new_ngo.verified,
        "verified flag must be preserved after owner transfer"
    );
    assert!(client.is_verified(&new_owner));
    assert_eq!(client.verified_count(), 1);

    assert_eq!(
        client.try_get_ngo(&old_owner),
        Err(Ok(Error::NotRegistered))
    );
    assert!(
        !client.is_verified(&old_owner),
        "old owner should not be verified after transfer"
    );
}

#[test]
fn ngo_owner_transfer_cancel_keeps_original_owner_verified() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    client.register(&old_owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &old_owner);
    let ngo_before = client.get_ngo(&old_owner);

    client.propose_owner_transfer(&old_owner, &new_owner);
    client.cancel_owner_transfer(&old_owner);

    assert!(client.is_verified(&old_owner));
    assert_eq!(client.get_ngo(&old_owner), ngo_before);
    assert_eq!(
        client.try_get_ngo(&new_owner),
        Err(Ok(Error::NotRegistered))
    );
    assert!(!client.is_verified(&new_owner));
    // A cancelled proposal can no longer be accepted.
    assert_eq!(
        client.try_accept_owner_transfer(&old_owner, &new_owner),
        Err(Ok(Error::NoPendingAdmin))
    );
}

#[test]
fn unverified_ngo_owner_transfer_keeps_unverified_status() {
    let (env, client, _admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    client.register(&old_owner, &String::from_str(&env, "Pending NGO"));

    client.propose_owner_transfer(&old_owner, &new_owner);
    client.accept_owner_transfer(&old_owner, &new_owner);

    let new_ngo = client.get_ngo(&new_owner);
    assert_eq!(new_ngo.owner, new_owner);
    assert!(
        !new_ngo.verified,
        "unverified status must be preserved after owner transfer"
    );
    assert!(!client.is_verified(&new_owner));
    assert_eq!(
        client.verification_status(&new_owner),
        VerificationStatus::Pending
    );
    assert_eq!(
        client.try_get_ngo(&old_owner),
        Err(Ok(Error::NotRegistered))
    );
}

#[test]
fn revoked_ngo_owner_transfer_keeps_revoked_status() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    client.register(&old_owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &old_owner);
    client.revoke_ngo(&admin, &old_owner);

    client.propose_owner_transfer(&old_owner, &new_owner);
    client.accept_owner_transfer(&old_owner, &new_owner);

    assert_eq!(
        client.verification_status(&new_owner),
        VerificationStatus::Revoked
    );
    assert_eq!(
        client.verification_status(&old_owner),
        VerificationStatus::Unknown
    );
}

#[test]
fn ngo_owner_transfer_preserves_all_ngo_metadata() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    client.register(
        &old_owner,
        &String::from_str(&env, "International Red Cross"),
    );
    client.set_metadata_uri(
        &old_owner,
        &Some(String::from_str(&env, "https://redcross.org/logo.png")),
    );
    client.set_categories(&old_owner, &(CATEGORY_HEALTH | CATEGORY_HUMANITAR));
    client.set_verify_period(&1_000);
    client.approve_ngo(&admin, &old_owner);
    let ngo_before = client.get_ngo(&old_owner);
    assert!(ngo_before.verified_until.is_some());

    client.propose_owner_transfer(&old_owner, &new_owner);
    client.accept_owner_transfer(&old_owner, &new_owner);

    assert_eq!(
        client.get_ngo(&new_owner),
        Ngo {
            owner: new_owner,
            pending_owner: None,
            ..ngo_before
        }
    );
}

#[test]
fn ngo_owner_transfer_moves_the_name_indexes_to_the_new_owner() {
    let (env, client, _admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let other = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");
    client.register(&old_owner, &name);

    client.propose_owner_transfer(&old_owner, &new_owner);
    client.accept_owner_transfer(&old_owner, &new_owner);

    assert_eq!(client.owner_by_name(&name), Some(new_owner.clone()));
    // The name stays claimed while the new owner holds it...
    assert_eq!(
        client.try_register(&other, &String::from_str(&env, "RED CROSS")),
        Err(Ok(Error::NameTaken))
    );

    // ...and is freed, in both indexes, once the new owner leaves.
    client.unregister(&new_owner);
    assert_eq!(client.owner_by_name(&name), None);
    client.register(&other, &name);
    assert_eq!(client.owner_by_name(&name), Some(other));
}

#[test]
fn ngo_owner_transfer_requires_old_owner_auth() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.register(&old_owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &old_owner);

    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "propose_owner_transfer",
            args: (old_owner.clone(), new_owner.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert!(client
        .try_propose_owner_transfer(&old_owner, &new_owner)
        .is_err());

    let ngo = client.get_ngo(&old_owner);
    assert_eq!(ngo.owner, old_owner);
    assert_eq!(ngo.pending_owner, None);
    assert!(ngo.verified);
}

#[test]
fn ngo_owner_transfer_requires_new_owner_auth_to_accept() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.register(&old_owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &old_owner);
    client.propose_owner_transfer(&old_owner, &new_owner);

    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "accept_owner_transfer",
            args: (old_owner.clone(), new_owner.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert!(client
        .try_accept_owner_transfer(&old_owner, &new_owner)
        .is_err());

    let ngo = client.get_ngo(&old_owner);
    assert_eq!(ngo.owner, old_owner);
    assert_eq!(ngo.pending_owner, Some(new_owner.clone()));
    assert_eq!(
        client.try_get_ngo(&new_owner),
        Err(Ok(Error::NotRegistered))
    );
}

#[test]
fn propose_owner_transfer_sets_pending_owner() {
    let (env, client, _admin) = setup();

    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    client.register(&old_owner, &name);
    client.propose_owner_transfer(&old_owner, &new_owner);
    assert_only_event(
        &env,
        &client,
        (symbol_short!("ownprop"), old_owner.clone()),
        (new_owner.clone(),),
    );

    let ngo = client.get_ngo(&old_owner);
    assert_eq!(ngo.pending_owner, Some(new_owner.clone()));
}

#[test]
fn accept_owner_transfer_moves_entry_and_preserves_verified() {
    let (env, client, admin) = setup();

    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    client.register(&old_owner, &name);
    client.approve_ngo(&admin, &old_owner);

    assert!(client.get_ngo(&old_owner).verified);

    client.propose_owner_transfer(&old_owner, &new_owner);
    client.accept_owner_transfer(&old_owner, &new_owner);
    assert_only_event(
        &env,
        &client,
        (symbol_short!("ownaccpt"), old_owner.clone()),
        (new_owner.clone(),),
    );

    // Old owner entry should be gone
    assert!(client.try_get_ngo(&old_owner).is_err());

    // New owner should have the entry with verified status preserved
    let ngo = client.get_ngo(&new_owner);
    assert_eq!(ngo.owner, new_owner);
    assert_eq!(ngo.name, name);
    assert!(ngo.verified);
    assert_eq!(ngo.pending_owner, None);
}

#[test]
fn accept_owner_transfer_requires_pending_match() {
    let (env, client, _admin) = setup();

    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let wrong_owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    client.register(&old_owner, &name);
    client.propose_owner_transfer(&old_owner, &new_owner);

    // Trying to accept with wrong address should fail
    let result = client.try_accept_owner_transfer(&old_owner, &wrong_owner);
    assert_eq!(result, Err(Ok(Error::NoPendingAdmin)));
}

#[test]
fn accept_owner_transfer_fails_if_new_owner_already_registered() {
    let (env, client, _admin) = setup();

    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let name1 = String::from_str(&env, "Test NGO 1");
    let name2 = String::from_str(&env, "Test NGO 2");

    client.register(&old_owner, &name1);
    client.register(&new_owner, &name2);

    client.propose_owner_transfer(&old_owner, &new_owner);

    let result = client.try_accept_owner_transfer(&old_owner, &new_owner);
    assert_eq!(result, Err(Ok(Error::AlreadyRegistered)));
}

#[test]
fn cancel_owner_transfer_clears_pending() {
    let (env, client, _admin) = setup();

    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    client.register(&old_owner, &name);
    client.propose_owner_transfer(&old_owner, &new_owner);

    assert!(client.get_ngo(&old_owner).pending_owner.is_some());

    client.cancel_owner_transfer(&old_owner);

    assert_only_event(
        &env,
        &client,
        (symbol_short!("owncancel"), old_owner.clone()),
        (),
    );

    assert_eq!(client.get_ngo(&old_owner).pending_owner, None);
}

#[test]
fn cancel_owner_transfer_without_pending_fails() {
    let (env, client, _admin) = setup();

    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    client.register(&owner, &name);

    let result = client.try_cancel_owner_transfer(&owner);
    assert_eq!(result, Err(Ok(Error::NoPendingAdmin)));
}

#[test]
fn propose_owner_transfer_to_self_fails() {
    let (env, client, _admin) = setup();

    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    client.register(&owner, &name);

    let result = client.try_propose_owner_transfer(&owner, &owner);
    assert_eq!(result, Err(Ok(Error::InvalidAdmin)));
}

#[test]
fn propose_owner_transfer_replaces_previous_proposal() {
    let (env, client, _admin) = setup();

    let owner = Address::generate(&env);
    let new_owner1 = Address::generate(&env);
    let new_owner2 = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    client.register(&owner, &name);
    client.propose_owner_transfer(&owner, &new_owner1);
    client.propose_owner_transfer(&owner, &new_owner2);

    let ngo = client.get_ngo(&owner);
    assert_eq!(ngo.pending_owner, Some(new_owner2));
}

#[test]
fn accept_owner_transfer_preserves_metadata_uri() {
    let (env, client, _admin) = setup();

    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");
    let metadata_uri = Some(String::from_str(&env, "https://example.com"));

    client.register(&old_owner, &name);
    client.set_metadata_uri(&old_owner, &metadata_uri);

    client.propose_owner_transfer(&old_owner, &new_owner);
    client.accept_owner_transfer(&old_owner, &new_owner);

    let ngo = client.get_ngo(&new_owner);
    assert_eq!(ngo.metadata_uri, metadata_uri);
}

#[test]
fn ownership_transfer_updates_index_correctly() {
    let (env, client, _admin) = setup();

    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    client.register(&old_owner, &name);

    // Check old owner is in the list
    let owners = client.list_ngos(&0, &10);
    assert!(owners.contains(&old_owner));

    client.propose_owner_transfer(&old_owner, &new_owner);
    client.accept_owner_transfer(&old_owner, &new_owner);

    // New owner should be in the list, old owner should not
    let owners = client.list_ngos(&0, &10);
    assert!(owners.contains(&new_owner));
    assert!(!owners.contains(&old_owner));
}
