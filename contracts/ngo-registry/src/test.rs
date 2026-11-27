// SPDX-License-Identifier: Apache-2.0
#![cfg(test)]

use super::*;
use soroban_sdk::testutils::storage::{Instance as _, Persistent as _};
use soroban_sdk::testutils::{
    Address as _, AuthorizedFunction, Events as _, Ledger, MockAuth, MockAuthInvoke,
};
use soroban_sdk::{symbol_short, vec, IntoVal, Symbol, TryFromVal, Val, Vec};

fn setup() -> (Env, NgoRegistryClient<'static>, Address) {
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
fn register_whitespace_only_name_fails() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let whitespace = String::from_str(&env, " \t\n\r");

    let result = client.try_register(&owner, &whitespace);

    assert_eq!(result, Err(Ok(Error::InvalidName)));
    assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
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

// =============================================================================
// metadata URI (issue #347)
// =============================================================================

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
            country_code: None,
        }
    );
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
fn get_unregistered_ngo_fails() {
    let (env, client, _admin) = setup();
    let random = Address::generate(&env);

    let result = client.try_get_ngo(&random);
    assert_eq!(result, Err(Ok(Error::NotRegistered)));
}

#[test]
fn is_verified_returns_false_for_registered_unverified_ngo() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    assert!(!client.is_verified(&owner));
}

#[test]
fn is_verified_returns_true_for_verified_ngo() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    assert!(client.is_verified(&owner));
}

#[test]
fn is_verified_returns_false_for_unknown_address() {
    let (env, client, _admin) = setup();
    let random = Address::generate(&env);

    assert!(!client.is_verified(&random));
}

#[test]
fn is_verified_returns_false_for_revoked_ngo() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    assert!(client.is_verified(&owner));

    client.revoke_ngo(&admin, &owner);
    assert!(!client.is_verified(&owner));
}

#[test]
fn approve_ngo_marks_verified() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");
    client.register(&owner, &name);

    client.approve_ngo(&admin, &owner);

    let ngo = client.get_ngo(&owner);
    assert!(ngo.verified);
}

#[test]
fn approve_ngo_twice_fails() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");
    client.register(&owner, &name);

    client.approve_ngo(&admin, &owner);

    let result = client.try_approve_ngo(&admin, &owner);
    assert_eq!(result, Err(Ok(Error::AlreadyVerified)));
    assert!(client.get_ngo(&owner).verified);
}

#[test]
fn approve_unregistered_ngo_fails() {
    let (env, client, admin) = setup();
    let random = Address::generate(&env);

    let result = client.try_approve_ngo(&admin, &random);
    assert_eq!(result, Err(Ok(Error::NotRegistered)));
}

// =============================================================================
// batch_approve (issue #275)
// =============================================================================

#[test]
fn batch_approve_approves_each_ngo_and_publishes_events() {
    let (env, client, admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    client.register(&a, &String::from_str(&env, "Red Cross"));
    client.register(&b, &String::from_str(&env, "Oxfam"));
    client.register(&c, &String::from_str(&env, "Save the Children"));

    client.batch_approve(&admin, &vec![&env, a.clone(), b.clone(), c.clone()]);

    // One `approved` event per NGO, in batch order. Checked before any other
    // call, because `events().all()` only covers the most recent invocation.
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("approved"), a.clone()).into_val(&env),
                ().into_val(&env),
            ),
            (
                client.address.clone(),
                (symbol_short!("approved"), b.clone()).into_val(&env),
                ().into_val(&env),
            ),
            (
                client.address.clone(),
                (symbol_short!("approved"), c.clone()).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );

    assert!(client.get_ngo(&a).verified);
    assert!(client.get_ngo(&b).verified);
    assert!(client.get_ngo(&c).verified);
}

#[test]
fn batch_approve_reverts_atomically_on_unregistered_ngo() {
    let (env, client, admin) = setup();
    let registered = Address::generate(&env);
    let unknown = Address::generate(&env);
    client.register(&registered, &String::from_str(&env, "Red Cross"));

    let result = client.try_batch_approve(&admin, &vec![&env, registered.clone(), unknown]);

    assert_eq!(result, Err(Ok(Error::NotRegistered)));
    // The whole batch rolls back: the registered NGO is still pending.
    assert!(!client.get_ngo(&registered).verified);
}

#[test]
fn batch_approve_reverts_atomically_on_already_verified_ngo() {
    let (env, client, admin) = setup();
    let verified = Address::generate(&env);
    let pending = Address::generate(&env);
    client.register(&verified, &String::from_str(&env, "Red Cross"));
    client.register(&pending, &String::from_str(&env, "Oxfam"));
    client.approve_ngo(&admin, &verified);

    let result = client.try_batch_approve(&admin, &vec![&env, pending.clone(), verified.clone()]);

    assert_eq!(result, Err(Ok(Error::AlreadyVerified)));
    assert!(!client.get_ngo(&pending).verified);
    assert!(client.get_ngo(&verified).verified);
}

#[test]
fn batch_approve_rejects_a_duplicate_owner() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    let result = client.try_batch_approve(&admin, &vec![&env, owner.clone(), owner.clone()]);

    assert_eq!(result, Err(Ok(Error::AlreadyVerified)));
    assert!(!client.get_ngo(&owner).verified);
}

#[test]
fn batch_approve_empty_batch_is_a_noop() {
    let (env, client, admin) = setup();
    let empty: Vec<Address> = Vec::new(&env);

    assert_eq!(client.try_batch_approve(&admin, &empty), Ok(Ok(())));
    assert_eq!(env.events().all().events().len(), 0);
}

#[test]
fn batch_approve_bumps_instance_and_ngo_ttl() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    age_past_thresholds(&env, &client, &owner);

    client.batch_approve(&admin, &vec![&env, owner.clone()]);

    assert_ttls_bumped(&env, &client, &owner);
}

#[test]
#[should_panic]
fn batch_approve_without_admin_auth_fails() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    env.mock_auths(&[]);
    client.batch_approve(&admin, &vec![&env, owner]);
}

#[test]
fn revoke_ngo_clears_verified_status() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");
    client.register(&owner, &name);

    client.approve_ngo(&admin, &owner);
    assert!(client.get_ngo(&owner).verified);

    client.revoke_ngo(&admin, &owner);

    let ngo = client.get_ngo(&owner);
    assert!(!ngo.verified);
}

#[test]
fn revoke_unverified_ngo_fails() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");
    client.register(&owner, &name);

    let result = client.try_revoke_ngo(&admin, &owner);
    assert_eq!(result, Err(Ok(Error::NotVerified)));
    assert!(!client.get_ngo(&owner).verified);
}

#[test]
fn revoke_unregistered_ngo_fails() {
    let (env, client, admin) = setup();
    let random = Address::generate(&env);

    let result = client.try_revoke_ngo(&admin, &random);
    assert_eq!(result, Err(Ok(Error::NotRegistered)));
}

#[test]
fn full_lifecycle_approve_revoke_reapprove() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Red Cross");

    // 1. Register NGO
    client.register(&owner, &name);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("register"), owner.clone()).into_val(&env),
                name.clone().into_val(&env),
            ),
        ]
    );
    let ngo = client.get_ngo(&owner);
    assert_eq!(ngo.owner, owner);
    assert_eq!(ngo.name, name);
    assert!(!ngo.verified);

    // 2. Approve NGO
    client.approve_ngo(&admin, &owner);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("approved"), owner.clone()).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );
    let ngo = client.get_ngo(&owner);
    assert_eq!(ngo.owner, owner);
    assert_eq!(ngo.name, name);
    assert!(ngo.verified);

    // 3. Revoke NGO
    client.revoke_ngo(&admin, &owner);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("revoked"), owner.clone()).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );
    let ngo = client.get_ngo(&owner);
    assert_eq!(ngo.owner, owner);
    assert_eq!(ngo.name, name);
    assert!(!ngo.verified);

    // 4. Reapprove NGO
    client.approve_ngo(&admin, &owner);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("approved"), owner.clone()).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );
    let ngo = client.get_ngo(&owner);
    assert_eq!(ngo.owner, owner);
    assert_eq!(ngo.name, name);
    assert!(ngo.verified);
}

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
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    age_past_thresholds(&env, &client, &owner);

    client.approve_ngo(&admin, &owner);

    assert_ttls_bumped(&env, &client, &owner);
}

#[test]
fn revoke_ngo_bumps_instance_and_ngo_ttl() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    age_past_thresholds(&env, &client, &owner);

    client.revoke_ngo(&admin, &owner);

    assert_ttls_bumped(&env, &client, &owner);
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
            country_code: None,
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
            country_code: None,
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
fn touch_ngo_bumps_instance_and_ngo_ttl() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    age_past_thresholds(&env, &client, &owner);

    client.touch_ngo(&owner);

    assert_ttls_bumped(&env, &client, &owner);
}

// --- Event assertion tests (issue #62) ---

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
fn approve_ngo_publishes_event() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.approve_ngo(&admin, &owner);

    // Events are cleared per top-level call; only the approved event is visible.
    assert_eq!(
        env.events().all(),
        soroban_sdk::vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("approved"), owner).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );
}

#[test]
fn revoke_ngo_publishes_event() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    client.revoke_ngo(&admin, &owner);

    // Events are cleared per top-level call; only the revoked event is visible.
    assert_eq!(
        env.events().all(),
        soroban_sdk::vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("revoked"), owner).into_val(&env),
                ().into_val(&env),
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

// list_ngos pagination tests (issue #283)
// =============================================================================

#[test]
fn list_ngos_empty_registry_returns_empty_list() {
    let (env, client, _admin) = setup();
    let result = client.list_ngos(&0, &10);
    assert_eq!(result.len(), 0);
    assert_eq!(result, Vec::new(&env));
}

#[test]
fn list_ngos_one_ngo() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Only NGO"));

    let result = client.list_ngos(&0, &10);
    assert_eq!(result.len(), 1);
    assert_eq!(result.get(0).unwrap(), owner);
}

#[test]
fn list_ngos_across_page_boundaries() {
    let (env, client, _admin) = setup();
    let mut owners = soroban_sdk::vec![&env];
    for _ in 0..5 {
        let owner = Address::generate(&env);
        let name = String::from_str(&env, "NGO");
        client.register(&owner, &name);
        owners.push_back(owner);
    }

    // Page 1: start = 0, limit = 2 -> owners [0, 1]
    let page1 = client.list_ngos(&0, &2);
    assert_eq!(page1.len(), 2);
    assert_eq!(page1.get(0).unwrap(), owners.get(0).unwrap());
    assert_eq!(page1.get(1).unwrap(), owners.get(1).unwrap());

    // Page 2: start = 2, limit = 2 -> owners [2, 3]
    let page2 = client.list_ngos(&2, &2);
    assert_eq!(page2.len(), 2);
    assert_eq!(page2.get(0).unwrap(), owners.get(2).unwrap());
    assert_eq!(page2.get(1).unwrap(), owners.get(3).unwrap());

    // Page 3: start = 4, limit = 2 -> owners [4]
    let page3 = client.list_ngos(&4, &2);
    assert_eq!(page3.len(), 1);
    assert_eq!(page3.get(0).unwrap(), owners.get(4).unwrap());
}

#[test]
fn list_ngos_start_beyond_end_returns_empty_list() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "NGO"));

    let result = client.list_ngos(&10, &5);
    assert_eq!(result.len(), 0);
}

#[test]
fn list_ngos_limit_larger_than_cap_is_clamped_to_max_limit() {
    let (env, client, _admin) = setup();
    // Register 55 NGOs (> MAX_LIST_LIMIT = 50)
    let mut registered = soroban_sdk::vec![&env];
    for _ in 0..55 {
        let owner = Address::generate(&env);
        client.register(&owner, &String::from_str(&env, "NGO"));
        registered.push_back(owner);
    }

    // Request limit = 100, must be clamped to 50
    let page = client.list_ngos(&0, &100);
    assert_eq!(page.len(), MAX_LIST_LIMIT);
    for i in 0..MAX_LIST_LIMIT {
        assert_eq!(page.get(i).unwrap(), registered.get(i).unwrap());
    }

    // Remaining 5 can be fetched in next page
    let remaining = client.list_ngos(&MAX_LIST_LIMIT, &50);
    assert_eq!(remaining.len(), 5);
    for i in 0..5 {
        assert_eq!(
            remaining.get(i).unwrap(),
            registered.get(MAX_LIST_LIMIT + i).unwrap()
        );
    }
}

#[test]
fn list_ngos_unregister_removes_from_middle_and_maintains_index() {
    let (env, client, _admin) = setup();
    let owner_a = Address::generate(&env);
    let owner_b = Address::generate(&env);
    let owner_c = Address::generate(&env);

    client.register(&owner_a, &String::from_str(&env, "NGO A"));
    client.register(&owner_b, &String::from_str(&env, "NGO B"));
    client.register(&owner_c, &String::from_str(&env, "NGO C"));

    let initial = client.list_ngos(&0, &10);
    assert_eq!(initial.len(), 3);
    assert_eq!(initial.get(0).unwrap(), owner_a);
    assert_eq!(initial.get(1).unwrap(), owner_b);
    assert_eq!(initial.get(2).unwrap(), owner_c);

    // Unregister B (middle of list). Swap-and-pop moves C into position 1.
    client.unregister(&owner_b);

    let after_unregister = client.list_ngos(&0, &10);
    assert_eq!(after_unregister.len(), 2);
    assert_eq!(after_unregister.get(0).unwrap(), owner_a);
    assert_eq!(after_unregister.get(1).unwrap(), owner_c);

    // B must not be listed
    assert_eq!(client.try_get_ngo(&owner_b), Err(Ok(Error::NotRegistered)));
}

#[test]
fn list_ngos_re_registering_after_unregister_has_no_duplicates() {
    let (env, client, _admin) = setup();
    let owner_a = Address::generate(&env);
    let owner_b = Address::generate(&env);
    let owner_c = Address::generate(&env);

    client.register(&owner_a, &String::from_str(&env, "NGO A"));
    client.register(&owner_b, &String::from_str(&env, "NGO B"));
    client.register(&owner_c, &String::from_str(&env, "NGO C"));

    // Unregister B
    client.unregister(&owner_b);

    // Re-register B
    client.register(&owner_b, &String::from_str(&env, "NGO B New"));

    let result = client.list_ngos(&0, &10);
    assert_eq!(result.len(), 3);
    assert_eq!(result.get(0).unwrap(), owner_a);
    assert_eq!(result.get(1).unwrap(), owner_c);
    assert_eq!(result.get(2).unwrap(), owner_b);
}

#[test]
fn list_ngos_unregister_last_and_only_entry() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Solo NGO"));

    assert_eq!(client.list_ngos(&0, &10).len(), 1);

    client.unregister(&owner);

    assert_eq!(client.list_ngos(&0, &10).len(), 0);
}

// =============================================================================
// list_ngos_page (issue #351)
// =============================================================================

#[test]
fn list_ngos_page_first_page_returns_next_start() {
    let (env, client, _admin) = setup();
    let mut owners = soroban_sdk::vec![&env];
    for _ in 0..5 {
        let owner = Address::generate(&env);
        client.register(&owner, &String::from_str(&env, "NGO"));
        owners.push_back(owner);
    }

    let page = client.list_ngos_page(&0, &2);

    assert_eq!(page.owners.len(), 2);
    assert_eq!(page.owners.get(0).unwrap(), owners.get(0).unwrap());
    assert_eq!(page.owners.get(1).unwrap(), owners.get(1).unwrap());
    assert_eq!(page.next, 2);
}

#[test]
fn list_ngos_page_last_page_next_reaches_total() {
    let (env, client, _admin) = setup();
    let mut owners = soroban_sdk::vec![&env];
    for _ in 0..5 {
        let owner = Address::generate(&env);
        client.register(&owner, &String::from_str(&env, "NGO"));
        owners.push_back(owner);
    }

    // Final, short page: one owner left and `next` lands on the total, so the
    // end of the list is distinguishable from a page that merely looks short.
    let page = client.list_ngos_page(&4, &2);

    assert_eq!(page.owners.len(), 1);
    assert_eq!(page.owners.get(0).unwrap(), owners.get(4).unwrap());
    assert_eq!(page.next, client.total_ngos());
}

#[test]
fn list_ngos_page_full_final_page_next_reaches_total() {
    let (env, client, _admin) = setup();
    for _ in 0..4 {
        let owner = Address::generate(&env);
        client.register(&owner, &String::from_str(&env, "NGO"));
    }

    // A page that exactly fills the limit can still be the last: `next` hits
    // the total, so the caller needs no extra (empty) call to discover the end.
    let page = client.list_ngos_page(&2, &2);

    assert_eq!(page.owners.len(), 2);
    assert_eq!(page.next, client.total_ngos());
}

#[test]
fn list_ngos_page_out_of_range_start_returns_empty_with_total() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "NGO"));

    let page = client.list_ngos_page(&10, &5);

    assert_eq!(page.owners.len(), 0);
    assert_eq!(page.owners, Vec::new(&env));
    assert_eq!(page.next, client.total_ngos());
}

#[test]
fn list_ngos_page_empty_registry_returns_total_zero() {
    let (_env, client, _admin) = setup();

    let page = client.list_ngos_page(&0, &10);

    assert_eq!(page.owners.len(), 0);
    assert_eq!(page.next, 0);
}

#[test]
fn list_ngos_page_walks_whole_index_via_next() {
    let (env, client, _admin) = setup();
    let mut owners = soroban_sdk::vec![&env];
    for _ in 0..7 {
        let owner = Address::generate(&env);
        client.register(&owner, &String::from_str(&env, "NGO"));
        owners.push_back(owner);
    }

    // Follow `next` to the end; every owner is visited exactly once, in order.
    let mut seen = soroban_sdk::vec![&env];
    let mut start = 0u32;
    loop {
        let page = client.list_ngos_page(&start, &3);
        for i in 0..page.owners.len() {
            seen.push_back(page.owners.get(i).unwrap());
        }
        start = page.next;
        if start >= client.total_ngos() {
            break;
        }
    }

    assert_eq!(seen, owners);
}

// =============================================================================
// verified_count (issue #352)
// =============================================================================

#[test]
fn verified_count_starts_at_zero() {
    let (_env, client, _admin) = setup();

    assert_eq!(client.verified_count(), 0);
}

#[test]
fn verified_count_increments_on_approve() {
    let (env, client, admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.register(&a, &String::from_str(&env, "A"));
    client.register(&b, &String::from_str(&env, "B"));

    client.approve_ngo(&admin, &a);
    assert_eq!(client.verified_count(), 1);
    client.approve_ngo(&admin, &b);
    assert_eq!(client.verified_count(), 2);
}

#[test]
fn verified_count_tracks_batch_approve() {
    let (env, client, admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    client.register(&a, &String::from_str(&env, "A"));
    client.register(&b, &String::from_str(&env, "B"));
    client.register(&c, &String::from_str(&env, "C"));

    client.batch_approve(&admin, &vec![&env, a, b, c]);

    assert_eq!(client.verified_count(), 3);
}

#[test]
fn verified_count_decrements_on_revoke() {
    let (env, client, admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.register(&a, &String::from_str(&env, "A"));
    client.register(&b, &String::from_str(&env, "B"));
    client.approve_ngo(&admin, &a);
    client.approve_ngo(&admin, &b);

    client.revoke_ngo(&admin, &a);
    assert_eq!(client.verified_count(), 1);
    client.revoke_ngo(&admin, &b);
    assert_eq!(client.verified_count(), 0);
}

#[test]
fn verified_count_unaffected_by_removing_an_unverified_ngo() {
    let (env, client, admin) = setup();
    let verified = Address::generate(&env);
    let pending = Address::generate(&env);
    client.register(&verified, &String::from_str(&env, "Verified"));
    client.register(&pending, &String::from_str(&env, "Pending"));
    client.approve_ngo(&admin, &verified);

    // Admin removal and owner-initiated unregister both drop unverified
    // entries only, so the verified tally is untouched.
    client.remove_ngo(&pending);
    assert_eq!(client.verified_count(), 1);

    let other = Address::generate(&env);
    client.register(&other, &String::from_str(&env, "Other"));
    client.unregister(&other);
    assert_eq!(client.verified_count(), 1);
}

#[test]
fn verified_count_decrements_when_a_verified_ngo_is_removed() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    assert_eq!(client.verified_count(), 1);

    // No public entry point removes a verified NGO — unregister, remove_ngo and
    // reject_ngo all refuse it — so drive the shared delete helper directly to
    // prove it keeps the counter honest.
    env.as_contract(&client.address, || {
        delete_ngo_entry(&env, &owner).unwrap();
    });

    assert_eq!(client.verified_count(), 0);
    assert_eq!(client.ngo_count(), 0);
}

#[test]
fn removing_a_verified_ngo_is_rejected_and_leaves_count_unchanged() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    assert_eq!(
        client.try_remove_ngo(&owner),
        Err(Ok(Error::AlreadyVerified))
    );
    assert_eq!(
        client.try_unregister(&owner),
        Err(Ok(Error::AlreadyVerified))
    );
    assert_eq!(
        client.try_reject_ngo(&admin, &owner, &1),
        Err(Ok(Error::AlreadyVerified))
    );
    assert_eq!(client.verified_count(), 1);
}

// =============================================================================
// verification expiry and renewal (issue #348)
// =============================================================================

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

#[test]
fn approve_without_period_never_expires() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    assert_eq!(client.get_ngo(&owner).verified_until, None);
    assert!(client.is_verified(&owner));

    // Without a period the verification is permanent, however far time moves.
    env.ledger().with_mut(|l| l.timestamp = 10_000_000);
    assert!(client.is_verified(&owner));
}

#[test]
fn approve_stamps_verified_until_from_period() {
    let (env, client, admin) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.set_verify_period(&500);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    assert_eq!(client.get_ngo(&owner).verified_until, Some(1_500));
}

#[test]
fn is_verified_holds_until_the_boundary_then_lapses() {
    let (env, client, admin) = setup();
    client.set_verify_period(&1_000);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    assert_eq!(client.get_ngo(&owner).verified_until, Some(1_000));

    // One second before the expiry the entry is still verified.
    env.ledger().with_mut(|l| l.timestamp = 999);
    assert!(client.is_verified(&owner));

    // At the expiry exactly, and after it, it is treated as unverified.
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    assert!(!client.is_verified(&owner));
    env.ledger().with_mut(|l| l.timestamp = 1_001);
    assert!(!client.is_verified(&owner));
}

#[test]
fn get_ngo_reports_unverified_after_expiry() {
    let (env, client, admin) = setup();
    client.set_verify_period(&1_000);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let ngo = client.get_ngo(&owner);
    assert!(!ngo.verified);
    // The stored expiry is kept so an admin can still renew the entry.
    assert_eq!(ngo.verified_until, Some(1_000));
}

#[test]
fn renew_ngo_revives_a_lapsed_verification_and_emits_event() {
    let (env, client, admin) = setup();
    client.set_verify_period(&1_000);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    // Time moves well past the original expiry.
    env.ledger().with_mut(|l| l.timestamp = 4_000);
    assert!(!client.is_verified(&owner));

    client.renew_ngo(&owner);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("renew"), owner.clone()).into_val(&env),
                5_000u64.into_val(&env),
            ),
        ]
    );
    assert!(client.is_verified(&owner));
    assert_eq!(client.get_ngo(&owner).verified_until, Some(5_000));
}

#[test]
fn renew_ngo_requires_admin_auth() {
    let (env, client, admin) = setup();
    client.set_verify_period(&1_000);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    client.renew_ngo(&owner);

    assert_sole_auth(&env, &client, &admin, "renew_ngo");
}

#[test]
fn renew_ngo_for_unregistered_ngo_fails() {
    let (env, client, _admin) = setup();
    client.set_verify_period(&1_000);
    let random = Address::generate(&env);

    assert_eq!(client.try_renew_ngo(&random), Err(Ok(Error::NotRegistered)));
}

#[test]
fn renew_ngo_for_unapproved_ngo_fails() {
    let (env, client, _admin) = setup();
    client.set_verify_period(&1_000);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    assert_eq!(client.try_renew_ngo(&owner), Err(Ok(Error::NotVerified)));
}

#[test]
fn renew_ngo_without_configured_period_fails() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    // No period is configured, so there is no expiry to extend.
    assert_eq!(client.try_renew_ngo(&owner), Err(Ok(Error::NoVerifyPeriod)));
}

#[test]
fn renew_ngo_after_revoke_fails() {
    let (env, client, admin) = setup();
    client.set_verify_period(&1_000);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    client.revoke_ngo(&admin, &owner);

    assert_eq!(client.try_renew_ngo(&owner), Err(Ok(Error::NotVerified)));
}

#[test]
fn revoke_clears_verified_until() {
    let (env, client, admin) = setup();
    client.set_verify_period(&1_000);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    assert_eq!(client.get_ngo(&owner).verified_until, Some(1_000));

    client.revoke_ngo(&admin, &owner);

    assert_eq!(client.get_ngo(&owner).verified_until, None);
}

#[test]
fn renew_ngo_bumps_instance_and_ngo_ttl() {
    let (env, client, admin) = setup();
    client.set_verify_period(&1_000);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    age_past_thresholds(&env, &client, &owner);

    client.renew_ngo(&owner);

    assert_ttls_bumped(&env, &client, &owner);
}

// =============================================================================
// register and unregister auth and events (issue #294)
// =============================================================================

/// Asserts that the last call recorded exactly one authorization: `expected`
/// authorizing `function` on the registry itself.
fn assert_sole_auth(env: &Env, client: &NgoRegistryClient, expected: &Address, function: &str) {
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
// NgoCount accuracy tests (issue #265)
// =============================================================================

#[test]
fn register_unregister_register_maintains_count() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    assert_eq!(client.ngo_count(), 0);

    client.register(&owner, &name);
    assert_eq!(client.ngo_count(), 1);

    client.unregister(&owner);
    assert_eq!(client.ngo_count(), 0);

    client.register(&owner, &name);
    assert_eq!(client.ngo_count(), 1);
}

#[test]
fn two_registrations_one_unregistration_count() {
    let (env, client, _admin) = setup();
    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);

    client.register(&owner1, &String::from_str(&env, "NGO 1"));
    assert_eq!(client.ngo_count(), 1);

    client.register(&owner2, &String::from_str(&env, "NGO 2"));
    assert_eq!(client.ngo_count(), 2);

    client.unregister(&owner1);
    assert_eq!(client.ngo_count(), 1);

    client.remove_ngo(&owner2);
    assert_eq!(client.ngo_count(), 0);
}

#[test]
fn count_stays_consistent_after_deterministic_operations() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    let d = Address::generate(&env);

    assert_eq!(client.ngo_count(), 0);

    // Register a -> 1
    client.register(&a, &String::from_str(&env, "A"));
    assert_eq!(client.ngo_count(), 1);

    // Register b -> 2
    client.register(&b, &String::from_str(&env, "B"));
    assert_eq!(client.ngo_count(), 2);

    // Register c -> 3
    client.register(&c, &String::from_str(&env, "C"));
    assert_eq!(client.ngo_count(), 3);

    // Unregister b -> 2
    client.unregister(&b);
    assert_eq!(client.ngo_count(), 2);

    // Register d -> 3
    client.register(&d, &String::from_str(&env, "D"));
    assert_eq!(client.ngo_count(), 3);

    // Admin removes c -> 2
    client.remove_ngo(&c);
    assert_eq!(client.ngo_count(), 2);

    // Register b again -> 3
    client.register(&b, &String::from_str(&env, "B again"));
    assert_eq!(client.ngo_count(), 3);

    // Unregister a -> 2
    client.unregister(&a);
    assert_eq!(client.ngo_count(), 2);

    // Admin removes d -> 1
    client.remove_ngo(&d);
    assert_eq!(client.ngo_count(), 1);

    // Unregister b -> 0
    client.unregister(&b);
    assert_eq!(client.ngo_count(), 0);
}

#[test]
fn unregister_unknown_owner_does_not_change_count() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let unknown = Address::generate(&env);

    client.register(&owner, &String::from_str(&env, "Known NGO"));
    assert_eq!(client.ngo_count(), 1);

    let res_unreg = client.try_unregister(&unknown);
    assert_eq!(res_unreg, Err(Ok(Error::NotRegistered)));
    assert_eq!(client.ngo_count(), 1);

    let res_rem = client.try_remove_ngo(&unknown);
    assert_eq!(res_rem, Err(Ok(Error::NotRegistered)));
    assert_eq!(client.ngo_count(), 1);
}

#[test]
fn register_overflow_returns_arithmetic_overflow() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);

    // Set stored NgoCount to u64::MAX
    env.as_contract(&client.address, || {
        env.storage().instance().set(&DataKey::NgoCount, &u64::MAX);
    });

    let res = client.try_register(&owner, &String::from_str(&env, "Overflow NGO"));
    assert_eq!(res, Err(Ok(Error::ArithmeticOverflow)));
    assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
}

#[test]
fn decrement_at_zero_returns_arithmetic_overflow() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);

    client.register(&owner, &String::from_str(&env, "Test NGO"));
    assert_eq!(client.ngo_count(), 1);

    // Corrupt NgoCount to 0 in instance storage
    env.as_contract(&client.address, || {
        env.storage().instance().set(&DataKey::NgoCount, &0u64);
    });

    let res = client.try_unregister(&owner);
    assert_eq!(res, Err(Ok(Error::ArithmeticOverflow)));

    // Entry should survive because the operation aborted before deletion
    assert!(client.try_get_ngo(&owner).is_ok());

    let res_rem = client.try_remove_ngo(&owner);
    assert_eq!(res_rem, Err(Ok(Error::ArithmeticOverflow)));
    assert!(client.try_get_ngo(&owner).is_ok());
}

// ── pause_registrations / resume_registrations / registrations_paused ────────

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

#[test]
fn failed_register_leaves_count_unchanged() {
    let (env, client, _admin) = setup();
    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);

    client.register(&owner1, &String::from_str(&env, "NGO 1"));
    assert_eq!(client.ngo_count(), 1);

    // Double register fails
    let res_double = client.try_register(&owner1, &String::from_str(&env, "NGO 1 again"));
    assert_eq!(res_double, Err(Ok(Error::AlreadyRegistered)));
    assert_eq!(client.ngo_count(), 1);

    // Invalid name fails
    let res_inv = client.try_register(&owner2, &String::from_str(&env, "   "));
    assert_eq!(res_inv, Err(Ok(Error::InvalidName)));
    assert_eq!(client.ngo_count(), 1);

    // Name too long fails
    let too_long = String::from_str(&env, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let res_long = client.try_register(&owner2, &too_long);
    assert_eq!(res_long, Err(Ok(Error::NameTooLong)));
    assert_eq!(client.ngo_count(), 1);
}

// =============================================================================
// total_ngos alias tests (issue #264)
// =============================================================================

#[test]
fn total_ngos_alias_agrees_with_ngo_count() {
    let (env, client, _admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    // Initial state: both agree at zero
    assert_eq!(client.ngo_count(), 0);
    assert_eq!(client.total_ngos(), 0);

    // Register two NGOs: both getters agree
    client.register(&a, &String::from_str(&env, "NGO A"));
    assert_eq!(client.ngo_count(), 1);
    assert_eq!(client.total_ngos(), 1);

    client.register(&b, &String::from_str(&env, "NGO B"));
    assert_eq!(client.ngo_count(), 2);
    assert_eq!(client.total_ngos(), 2);

    // Unregister sequence: both getters agree after removal
    client.unregister(&a);
    assert_eq!(client.ngo_count(), 1);
    assert_eq!(client.total_ngos(), 1);

    // Admin removal: both getters agree back to zero
    client.remove_ngo(&b);
    assert_eq!(client.ngo_count(), 0);
    assert_eq!(client.total_ngos(), 0);
}

#[test]
fn total_ngos_alias_register_unregister_register_sequence() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "NGO Test");

    assert_eq!(client.total_ngos(), 0);
    assert_eq!(client.ngo_count(), 0);

    client.register(&owner, &name);
    assert_eq!(client.total_ngos(), 1);
    assert_eq!(client.ngo_count(), 1);

    client.unregister(&owner);
    assert_eq!(client.total_ngos(), 0);
    assert_eq!(client.ngo_count(), 0);

    client.register(&owner, &name);
    assert_eq!(client.total_ngos(), 1);
    assert_eq!(client.ngo_count(), 1);
}

// =============================================================================
// approver role (issue #349)
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
fn admin_can_review_without_being_listed() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    // The admin is not part of the explicit approver list...
    assert!(!client.is_approver(&admin));
    // ...but still approves and revokes as before.
    client.approve_ngo(&admin, &owner);
    assert!(client.get_ngo(&owner).verified);
    client.revoke_ngo(&admin, &owner);
    assert!(!client.get_ngo(&owner).verified);
}

#[test]
fn approver_can_approve_and_revoke() {
    let (env, client, _admin) = setup();
    let reviewer = Address::generate(&env);
    let owner = Address::generate(&env);
    client.add_approver(&reviewer);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.approve_ngo(&reviewer, &owner);
    // The approver, not the admin, is the sole authorizer. Check before any
    // other contract call replaces the recorded auths.
    assert_sole_auth(&env, &client, &reviewer, "approve_ngo");
    assert!(client.get_ngo(&owner).verified);

    client.revoke_ngo(&reviewer, &owner);
    assert_sole_auth(&env, &client, &reviewer, "revoke_ngo");
    assert!(!client.get_ngo(&owner).verified);
}

#[test]
fn approver_can_batch_approve() {
    let (env, client, _admin) = setup();
    let reviewer = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.add_approver(&reviewer);
    client.register(&a, &String::from_str(&env, "Red Cross"));
    client.register(&b, &String::from_str(&env, "Oxfam"));

    client.batch_approve(&reviewer, &vec![&env, a.clone(), b.clone()]);

    assert!(client.get_ngo(&a).verified);
    assert!(client.get_ngo(&b).verified);
}

#[test]
fn non_approver_cannot_approve() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    assert_eq!(
        client.try_approve_ngo(&stranger, &owner),
        Err(Ok(Error::NotApprover))
    );
    assert!(!client.get_ngo(&owner).verified);
}

#[test]
fn non_approver_cannot_batch_approve() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    assert_eq!(
        client.try_batch_approve(&stranger, &vec![&env, owner.clone()]),
        Err(Ok(Error::NotApprover))
    );
    assert!(!client.get_ngo(&owner).verified);
}

#[test]
fn non_approver_cannot_revoke() {
    let (env, client, admin) = setup();
    let stranger = Address::generate(&env);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    assert_eq!(
        client.try_revoke_ngo(&stranger, &owner),
        Err(Ok(Error::NotApprover))
    );
    assert!(client.get_ngo(&owner).verified);
}

#[test]
fn removed_approver_can_no_longer_review() {
    let (env, client, _admin) = setup();
    let reviewer = Address::generate(&env);
    let owner = Address::generate(&env);
    client.add_approver(&reviewer);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.remove_approver(&reviewer);

    assert_eq!(
        client.try_approve_ngo(&reviewer, &owner),
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
    assert!(client.get_ngo(&owner).verified == false);
}

// =============================================================================
// reject_ngo with a reason code (issue #350)
// =============================================================================

#[test]
fn reject_ngo_removes_pending_entry_and_emits_event() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let reason_code = 7u32;
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.reject_ngo(&admin, &owner, &reason_code);

    // Events are cleared per top-level call, so only the rejected event shows.
    // Assert before the state reads below, which would reset the buffer.
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("rejected"), owner.clone()).into_val(&env),
                reason_code.into_val(&env),
            ),
        ]
    );

    assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
    assert!(!client.is_verified(&owner));
    assert_eq!(client.total_ngos(), 0);
    assert_eq!(client.list_ngos(&0, &10).len(), 0);
}

#[test]
fn reject_ngo_publishes_distinct_reason_codes() {
    let (env, client, admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.register(&a, &String::from_str(&env, "A"));
    client.register(&b, &String::from_str(&env, "B"));

    client.reject_ngo(&admin, &a, &1);
    client.reject_ngo(&admin, &b, &99);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("rejected"), b).into_val(&env),
                99u32.into_val(&env),
            ),
        ]
    );
}

#[test]
fn reject_ngo_rejects_a_verified_ngo() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);

    let result = client.try_reject_ngo(&admin, &owner, &3);

    assert_eq!(result, Err(Ok(Error::AlreadyVerified)));
    // Untouched: revoke_ngo is the way to undo an approval.
    assert!(client.get_ngo(&owner).verified);
    assert_eq!(client.total_ngos(), 1);
}

#[test]
fn reject_ngo_after_revocation_succeeds() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    client.revoke_ngo(&admin, &owner);

    client.reject_ngo(&admin, &owner, &3);

    assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
    assert_eq!(client.total_ngos(), 0);
}

#[test]
fn reject_ngo_unknown_owner_fails() {
    let (env, client, admin) = setup();
    let unknown = Address::generate(&env);

    let result = client.try_reject_ngo(&admin, &unknown, &3);

    assert_eq!(result, Err(Ok(Error::NotRegistered)));
}

#[test]
fn admin_can_reject_without_being_listed() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    assert!(!client.is_approver(&admin));
    client.reject_ngo(&admin, &owner, &3);

    assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
}

#[test]
fn approver_can_reject() {
    let (env, client, _admin) = setup();
    let reviewer = Address::generate(&env);
    let owner = Address::generate(&env);
    client.add_approver(&reviewer);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    client.reject_ngo(&reviewer, &owner, &3);

    assert_sole_auth(&env, &client, &reviewer, "reject_ngo");
    assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
}

#[test]
fn non_approver_cannot_reject() {
    let (env, client, _admin) = setup();
    let outsider = Address::generate(&env);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    let result = client.try_reject_ngo(&outsider, &owner, &3);

    assert_eq!(result, Err(Ok(Error::NotApprover)));
    assert!(client.get_ngo(&owner).verified == false);
}

#[test]
fn removed_approver_can_no_longer_reject() {
    let (env, client, _admin) = setup();
    let reviewer = Address::generate(&env);
    let owner = Address::generate(&env);
    client.add_approver(&reviewer);
    client.remove_approver(&reviewer);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    let result = client.try_reject_ngo(&reviewer, &owner, &3);

    assert_eq!(result, Err(Ok(Error::NotApprover)));
}

#[test]
#[should_panic]
fn reject_ngo_without_auth_fails() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

    env.mock_auths(&[]);
    client.reject_ngo(&Address::generate(&env), &owner, &3);
}

#[test]
fn reject_ngo_frees_the_index_and_lets_the_owner_register_again() {
    let (env, client, admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    client.register(&a, &String::from_str(&env, "A"));
    client.register(&b, &String::from_str(&env, "B"));
    client.register(&c, &String::from_str(&env, "C"));

    client.reject_ngo(&admin, &b, &3);

    let owners = client.list_ngos(&0, &10);
    assert_eq!(owners.len(), 2);
    assert_eq!(owners.get(0).unwrap(), a);
    assert_eq!(owners.get(1).unwrap(), c);
    assert_eq!(client.total_ngos(), 2);

    // The rejected owner can apply again from scratch.
    client.register(&b, &String::from_str(&env, "B again"));
    assert!(!client.get_ngo(&b).verified);
    assert_eq!(client.total_ngos(), 3);
}

// =============================================================================
// verification_status (issue #448)
// =============================================================================

#[test]
fn verification_status_unknown_for_unregistered_address() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);
    assert_eq!(
        client.verification_status(&stranger),
        VerificationStatus::Unknown
    );
}

#[test]
fn verification_status_pending_after_register() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    assert_eq!(
        client.verification_status(&owner),
        VerificationStatus::Pending
    );
}

#[test]
fn verification_status_verified_after_approve() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    assert_eq!(
        client.verification_status(&owner),
        VerificationStatus::Verified
    );
}

#[test]
fn verification_status_revoked_after_revoke() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    client.revoke_ngo(&admin, &owner);
    assert_eq!(
        client.verification_status(&owner),
        VerificationStatus::Revoked
    );
}

#[test]
fn verification_status_expired_after_period_lapses() {
    let (env, client, admin) = setup();
    client.set_verify_period(&1_000);
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    assert_eq!(
        client.verification_status(&owner),
        VerificationStatus::Verified
    );

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    assert_eq!(
        client.verification_status(&owner),
        VerificationStatus::Expired
    );
}

#[test]
fn verification_status_verified_again_after_reapprove() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &owner);
    client.revoke_ngo(&admin, &owner);
    assert_eq!(
        client.verification_status(&owner),
        VerificationStatus::Revoked
    );

    client.approve_ngo(&admin, &owner);
    assert_eq!(
        client.verification_status(&owner),
        VerificationStatus::Verified
    );
}

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
    const SOURCE: &str = include_str!("lib.rs");
    const ROLES: &str = include_str!("../../../docs/ROLES.md");
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
// =============================================================================
// Test for list_ngos handling removed entries (Issue 4)
// =============================================================================

#[test]
#[ignore] // TODO: Requires propose_ngo_owner, accept_ngo_owner, cancel_ngo_owner_proposal implementation
fn ngo_owner_transfer_preserves_verified_flag_on_propose() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);

    // Register and verify an NGO
    client.register(&old_owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &old_owner);

    // Verify the NGO is verified before transfer
    assert!(client.is_verified(&old_owner));
    let ngo_before = client.get_ngo(&old_owner);
    assert!(ngo_before.verified);

    // Propose owner transfer (feature not yet implemented)
    // client.propose_ngo_owner(&old_owner, &new_owner);

    // After proposal, the old owner should still be verified and the entry unchanged
    assert!(client.is_verified(&old_owner));
    let ngo_after_propose = client.get_ngo(&old_owner);
    assert!(ngo_after_propose.verified);
    assert_eq!(ngo_after_propose.owner, old_owner);

    // The new owner should not have an NGO entry yet
    assert_eq!(
        client.try_get_ngo(&new_owner),
        Err(Ok(Error::NotRegistered))
    );
}

#[test]
#[ignore] // TODO: Requires propose_ngo_owner, accept_ngo_owner implementation
fn ngo_owner_transfer_preserves_verified_flag_on_accept() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);

    // Register and verify an NGO
    client.register(&old_owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &old_owner);

    // Verify the NGO is verified before transfer
    assert!(client.is_verified(&old_owner));

    // Propose and accept owner transfer (features not yet implemented)
    // client.propose_ngo_owner(&old_owner, &new_owner);
    // client.accept_ngo_owner(&new_owner);

    // After acceptance:
    // 1. The new owner should have the NGO with verified flag preserved
    let new_ngo = client.get_ngo(&new_owner);
    assert_eq!(new_ngo.owner, new_owner);
    assert!(
        new_ngo.verified,
        "verified flag must be preserved after owner transfer"
    );
    assert!(client.is_verified(&new_owner));

    // 2. The old owner should no longer have an NGO entry
    assert_eq!(
        client.try_get_ngo(&old_owner),
        Err(Ok(Error::NotRegistered))
    );
    assert!(
        !client.is_verified(&old_owner),
        "old owner should not be verified after transfer"
    );

    // 3. Existing streams pointing to the old owner address should still be valid
    // (This would be tested in donation-vault tests, but the registry transfer
    // must not break them by invalidating the NGO)
}

#[test]
#[ignore] // TODO: Requires propose_ngo_owner, cancel_ngo_owner_proposal implementation
fn ngo_owner_transfer_cancel_keeps_original_owner_verified() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);

    // Register and verify an NGO
    client.register(&old_owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &old_owner);

    // Verify the NGO is verified before transfer
    assert!(client.is_verified(&old_owner));
    let ngo_before = client.get_ngo(&old_owner);

    // Propose owner transfer (feature not yet implemented)
    // client.propose_ngo_owner(&old_owner, &new_owner);

    // Cancel the proposal (feature not yet implemented)
    // client.cancel_ngo_owner_proposal(&old_owner);

    // After cancellation:
    // 1. The old owner should still be verified with unchanged entry
    assert!(client.is_verified(&old_owner));
    let ngo_after_cancel = client.get_ngo(&old_owner);
    assert!(ngo_after_cancel.verified);
    assert_eq!(ngo_after_cancel.owner, old_owner);
    assert_eq!(ngo_after_cancel.name, ngo_before.name);

    // 2. The new owner should not have an NGO entry
    assert_eq!(
        client.try_get_ngo(&new_owner),
        Err(Ok(Error::NotRegistered))
    );
    assert!(!client.is_verified(&new_owner));
}

#[test]
#[ignore] // TODO: Requires propose_ngo_owner, accept_ngo_owner implementation
fn unverified_ngo_owner_transfer_keeps_unverified_status() {
    let (env, client, _admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);

    // Register but DO NOT verify the NGO
    client.register(&old_owner, &String::from_str(&env, "Pending NGO"));

    // Verify the NGO is NOT verified before transfer
    assert!(!client.is_verified(&old_owner));
    let ngo_before = client.get_ngo(&old_owner);
    assert!(!ngo_before.verified);

    // Propose and accept owner transfer (features not yet implemented)
    // client.propose_ngo_owner(&old_owner, &new_owner);
    // client.accept_ngo_owner(&new_owner);

    // After acceptance:
    // 1. The new owner should have the NGO with unverified status preserved
    let new_ngo = client.get_ngo(&new_owner);
    assert_eq!(new_ngo.owner, new_owner);
    assert!(
        !new_ngo.verified,
        "unverified status must be preserved after owner transfer"
    );
    assert!(!client.is_verified(&new_owner));

    // 2. The old owner should no longer have an NGO entry
    assert_eq!(
        client.try_get_ngo(&old_owner),
        Err(Ok(Error::NotRegistered))
    );
}

#[test]
#[ignore] // TODO: Requires propose_ngo_owner, accept_ngo_owner implementation
fn ngo_owner_transfer_preserves_all_ngo_metadata() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);

    // Register, verify, and set metadata for an NGO
    client.register(
        &old_owner,
        &String::from_str(&env, "International Red Cross"),
    );
    client.approve_ngo(&admin, &old_owner);
    client.set_metadata_uri(
        &old_owner,
        &Some(String::from_str(&env, "https://redcross.org/logo.png")),
    );

    let ngo_before = client.get_ngo(&old_owner);

    // Propose and accept owner transfer (features not yet implemented)
    // client.propose_ngo_owner(&old_owner, &new_owner);
    // client.accept_ngo_owner(&new_owner);

    // After acceptance, all metadata should be preserved on the new owner
    let new_ngo = client.get_ngo(&new_owner);
    assert_eq!(new_ngo.owner, new_owner);
    assert_eq!(new_ngo.name, ngo_before.name, "name must be preserved");
    assert_eq!(
        new_ngo.verified, ngo_before.verified,
        "verified flag must be preserved"
    );
    assert_eq!(
        new_ngo.metadata_uri, ngo_before.metadata_uri,
        "metadata_uri must be preserved"
    );
    assert_eq!(
        new_ngo.verified_until, ngo_before.verified_until,
        "verified_until must be preserved"
    );
}

#[test]
#[ignore] // TODO: Requires propose_ngo_owner implementation
fn ngo_owner_transfer_requires_old_owner_auth() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let attacker = Address::generate(&env);

    // Register and verify an NGO
    client.register(&old_owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &old_owner);

    // Attempt to propose owner transfer without proper auth (feature not yet implemented)
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "propose_ngo_owner",
            args: (old_owner.clone(), new_owner.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    // This should fail - only the old owner can initiate transfer
    // let result = client.try_propose_ngo_owner(&old_owner, &new_owner);
    // assert!(result.is_err());

    // The NGO should remain unchanged
    let ngo = client.get_ngo(&old_owner);
    assert_eq!(ngo.owner, old_owner);
    assert!(ngo.verified);
}

#[test]
#[ignore] // TODO: Requires propose_ngo_owner, accept_ngo_owner implementation
fn ngo_owner_transfer_requires_new_owner_auth_to_accept() {
    let (env, client, admin) = setup();
    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let attacker = Address::generate(&env);

    // Register and verify an NGO
    client.register(&old_owner, &String::from_str(&env, "Red Cross"));
    client.approve_ngo(&admin, &old_owner);

    // Propose owner transfer (feature not yet implemented)
    // client.propose_ngo_owner(&old_owner, &new_owner);

    // Attempt to accept with wrong auth (feature not yet implemented)
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "accept_ngo_owner",
            args: (new_owner.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    // This should fail - only the new owner can accept
    // let result = client.try_accept_ngo_owner(&new_owner);
    // assert!(result.is_err());

    // The NGO should remain with the old owner
    let ngo = client.get_ngo(&old_owner);
    assert_eq!(ngo.owner, old_owner);
}

#[test]
fn list_ngos_handles_removed_entries_correctly() {
    let (env, client, _admin) = setup();

    // Register 5 NGOs
    let mut owners: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
    let names = ["NGO 0", "NGO 1", "NGO 2", "NGO 3", "NGO 4"];
    for name_str in names.iter() {
        let owner = Address::generate(&env);
        client.register(&owner, &String::from_str(&env, name_str));
        owners.push_back(owner);
    }
    assert_eq!(client.ngo_count(), 5);

    // Get the initial list of all NGOs
    let initial_page = client.list_ngos_page(&0, &10);
    assert_eq!(initial_page.owners.len(), 5);
    assert_eq!(initial_page.next, 5);

    // Remove the NGO at index 2 (middle)
    let middle_owner = owners.get(2).unwrap();
    client.remove_ngo(&middle_owner);

    // Verify count decreased
    assert_eq!(client.ngo_count(), 4);

    // List NGOs after removal
    let after_removal_page = client.list_ngos_page(&0, &10);
    assert_eq!(after_removal_page.owners.len(), 4);
    assert_eq!(after_removal_page.next, 4);

    // Verify the removed NGO is not in the list
    assert!(!after_removal_page.owners.contains(&middle_owner));

    // Verify the other NGOs are still accessible; the removed one is not.
    for i in 0..5u32 {
        let owner = owners.get(i).unwrap();
        if i == 2 {
            assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
        } else {
            assert!(client.try_get_ngo(&owner).is_ok());
        }
    }

    // Pagination across the remaining 4 entries yields them all exactly once.
    let first_page = client.list_ngos_page(&0, &2);
    assert_eq!(first_page.owners.len(), 2);
    assert_eq!(first_page.next, 2);

    let second_page = client.list_ngos_page(&2, &2);
    assert_eq!(second_page.owners.len(), 2);
    assert_eq!(second_page.next, 4);

    let mut all_owners_after = first_page.owners.clone();
    all_owners_after.append(&second_page.owners);
    assert_eq!(all_owners_after.len(), 4);

    // Verify the removed owner is not present in either page.
    for i in 0..all_owners_after.len() {
        assert_ne!(all_owners_after.get(i).unwrap(), middle_owner);
    }
}

// ── owner_by_name (issue #447) ─────────────────────────────────────────────

fn name_of(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

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
fn upgrade_with_timelock_disabled_succeeds() {
    let (env, client, _admin) = setup();
    let new_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

    // Default upgrade_delay is 0, so immediate upgrade should work
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

    assert_eq!(client.upgrade_delay(), 3_600);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("updelay"),).into_val(&env),
                3_600u64.into_val(&env),
            ),
        ]
    );
}

#[test]
fn propose_upgrade_records_pending_and_publishes_event() {
    let (env, client, _admin) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.set_upgrade_delay(&3_600);

    let new_wasm_hash = BytesN::from_array(&env, &[7u8; 32]);

    client.propose_upgrade(&new_wasm_hash);

    let pending = client.pending_upgrade().expect("a proposal is pending");
    assert_eq!(pending.new_wasm_hash, new_wasm_hash);
    assert_eq!(pending.executable_at, 4_600);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("uppropose"), new_wasm_hash.clone()).into_val(&env),
                4_600u64.into_val(&env),
            ),
        ]
    );
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

    assert!(client.pending_upgrade().is_none());
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("upcancel"),).into_val(&env),
                new_wasm_hash.into_val(&env),
            ),
        ]
    );
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

#[test]
fn propose_owner_transfer_sets_pending_owner() {
    let (env, client, _admin) = setup();

    let old_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    client.register(&old_owner, &name);
    client.propose_owner_transfer(&old_owner, &new_owner);
    let events = env.events().all();

    let ngo = client.get_ngo(&old_owner);
    assert_eq!(ngo.pending_owner, Some(new_owner.clone()));

    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("ownprop"), old_owner.clone()).into_val(&env),
                new_owner.clone().into_val(&env),
            ),
        ]
    );
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
    let events = env.events().all();

    // Old owner entry should be gone
    assert!(client.try_get_ngo(&old_owner).is_err());

    // New owner should have the entry with verified status preserved
    let ngo = client.get_ngo(&new_owner);
    assert_eq!(ngo.owner, new_owner);
    assert_eq!(ngo.name, name);
    assert!(ngo.verified);
    assert_eq!(ngo.pending_owner, None);

    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("ownaccpt"), old_owner.clone()).into_val(&env),
                new_owner.clone().into_val(&env),
            ),
        ]
    );
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
    let events = env.events().all();

    assert_eq!(client.get_ngo(&old_owner).pending_owner, None);

    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("owncancel"), old_owner.clone()).into_val(&env),
                ().into_val(&env),
            ),
        ]
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

#[test]
fn register_bumps_instance_and_ngo_ttl_roles() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    age_past_thresholds(&env, &client, &owner);

    client.register(&owner, &String::from_str(&env, "Red Cross"));

    assert_ttls_bumped(&env, &client, &owner);
}

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

    assert_eq!(client.categories(&owner), mask);
    assert_eq!(client.get_ngo(&owner).categories, mask);

    assert_eq!(
        env.events().all(),
        soroban_sdk::vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("catset"), owner.clone()).into_val(&env),
                mask.into_val(&env),
            ),
        ]
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
            country_code: None,
        }
    );
}

#[test]
fn owner_by_name_keeps_the_first_claimant_when_names_collide() {
    let (env, client, _admin) = setup();
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    client.register(&first, &name_of(&env, "Red Cross"));
    client.register(&second, &name_of(&env, "Red Cross"));
    assert_eq!(
        client.owner_by_name(&name_of(&env, "Red Cross")),
        Some(first.clone())
    );

    // The unindexed owner leaving, or renaming, must not clear the first claim.
    client.update_name(&second, &name_of(&env, "Red Cross International"));
    assert_eq!(
        client.owner_by_name(&name_of(&env, "Red Cross")),
        Some(first.clone())
    );
    assert_eq!(
        client.owner_by_name(&name_of(&env, "Red Cross International")),
        Some(second.clone())
    );

    client.unregister(&second);
    assert_eq!(
        client.owner_by_name(&name_of(&env, "Red Cross")),
        Some(first)
    );
    assert_eq!(
        client.owner_by_name(&name_of(&env, "Red Cross International")),
        None
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

#[test]
#[ignore] // Remove this when owner transfer is implemented
fn ngo_owner_transfer_preserves_verified_flag() {
    let (env, client, admin) = setup();

    let old_owner = Address::generate(&env);
    let _new_owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    // Register and verify an NGO
    client.register(&old_owner, &name);
    client.approve_ngo(&admin, &old_owner);
    assert!(client.is_verified(&old_owner));

    // TODO: When implemented, uncomment these lines:
    // Propose owner transfer
    // client.propose_ngo_owner(&old_owner, &new_owner);

    // Accept transfer
    // client.accept_ngo_owner(&new_owner);

    // Verify the new owner is verified
    // assert!(client.is_verified(&new_owner));

    // Verify the old owner is no longer registered
    // assert!(!client.is_verified(&old_owner));
    // assert_eq!(client.try_get_ngo(&old_owner), Err(Ok(Error::NotRegistered)));
}

#[test]
#[ignore] // Remove this when owner transfer is implemented
fn ngo_owner_transfer_cancel_keeps_original_owner() {
    let (env, client, admin) = setup();

    let old_owner = Address::generate(&env);
    let _new_owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    // Register and verify an NGO
    client.register(&old_owner, &name);
    client.approve_ngo(&admin, &old_owner);

    // TODO: When implemented, uncomment these lines:
    // Propose owner transfer
    // client.propose_ngo_owner(&old_owner, &new_owner);

    // Cancel the proposal
    // client.cancel_ngo_owner_proposal(&old_owner);

    // Verify old owner still verified
    assert!(client.is_verified(&old_owner));

    // Verify new owner is not registered
    // assert!(!client.is_verified(&new_owner));
    // assert_eq!(client.try_get_ngo(&new_owner), Err(Ok(Error::NotRegistered)));
}

#[test]
#[ignore] // Remove this when owner transfer is implemented
fn ngo_owner_transfer_does_not_break_existing_streams() {
    let (env, client, admin) = setup();

    let old_owner = Address::generate(&env);
    let _new_owner = Address::generate(&env);
    let name = String::from_str(&env, "Test NGO");

    // Register and verify an NGO
    client.register(&old_owner, &name);
    client.approve_ngo(&admin, &old_owner);

    // TODO: When implemented, uncomment and test that:
    // 1. Existing donation streams to old_owner address still work
    // 2. After transfer, new streams can be created to new_owner
    // 3. Verification status transfers correctly

    // This test would need integration with donation-vault contract
    // to fully verify streams are not broken
}

// =============================================================================
// country code (issue #445)
// =============================================================================

#[test]
fn register_defaults_country_code_to_none() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));

    assert_eq!(client.get_ngo(&owner).country_code, None);
}

#[test]
fn set_country_code_stores_valid_code_and_publishes_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));

    let code = Some(String::from_str(&env, "NG"));
    client.set_country_code(&owner, &code);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("cntrycode"), owner.clone()).into_val(&env),
                code.clone().into_val(&env),
            ),
        ]
    );
    assert_eq!(client.get_ngo(&owner).country_code, code);
}

#[test]
fn set_country_code_clears_with_none() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));
    client.set_country_code(&owner, &Some(String::from_str(&env, "NG")));

    client.set_country_code(&owner, &None);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("cntrycode"), owner.clone()).into_val(&env),
                None::<String>.into_val(&env),
            ),
        ]
    );
    assert_eq!(client.get_ngo(&owner).country_code, None);
}

#[test]
fn set_country_code_no_event_when_unchanged() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));
    let code = Some(String::from_str(&env, "GB"));
    client.set_country_code(&owner, &code);

    // Setting the same value again must not emit an event.
    client.set_country_code(&owner, &code);

    assert_eq!(env.events().all().events().len(), 0);
}

#[test]
fn set_country_code_rejects_lowercase() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));

    let result = client.try_set_country_code(&owner, &Some(String::from_str(&env, "ng")));

    assert_eq!(result, Err(Ok(Error::InvalidCountryCode)));
    assert_eq!(client.get_ngo(&owner).country_code, None);
}

#[test]
fn set_country_code_rejects_mixed_case() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));

    let result = client.try_set_country_code(&owner, &Some(String::from_str(&env, "Ng")));

    assert_eq!(result, Err(Ok(Error::InvalidCountryCode)));
}

#[test]
fn set_country_code_rejects_single_letter() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));

    let result = client.try_set_country_code(&owner, &Some(String::from_str(&env, "N")));

    assert_eq!(result, Err(Ok(Error::InvalidCountryCode)));
}

#[test]
fn set_country_code_rejects_three_letters() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));

    let result = client.try_set_country_code(&owner, &Some(String::from_str(&env, "NGA")));

    assert_eq!(result, Err(Ok(Error::InvalidCountryCode)));
}

#[test]
fn set_country_code_rejects_digits() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));

    let result = client.try_set_country_code(&owner, &Some(String::from_str(&env, "N1")));

    assert_eq!(result, Err(Ok(Error::InvalidCountryCode)));
}

#[test]
fn set_country_code_rejects_empty_string() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));

    let result = client.try_set_country_code(&owner, &Some(String::from_str(&env, "")));

    assert_eq!(result, Err(Ok(Error::InvalidCountryCode)));
}

#[test]
fn set_country_code_for_unregistered_ngo_fails() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);

    let result = client.try_set_country_code(&stranger, &Some(String::from_str(&env, "NG")));

    assert_eq!(result, Err(Ok(Error::NotRegistered)));
}

#[test]
fn set_country_code_requires_owner_auth() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));
    client.set_country_code(&owner, &Some(String::from_str(&env, "NG")));

    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    let (address, invocation) = &auths[0];
    assert_eq!(address, &owner);
    match &invocation.function {
        AuthorizedFunction::Contract((contract, function, _)) => {
            assert_eq!(contract, &client.address);
            assert_eq!(function, &Symbol::new(&env, "set_country_code"));
        }
        _ => panic!("expected a contract invocation"),
    }
}

#[test]
fn set_country_code_allowed_after_verification() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));
    client.approve_ngo(&admin, &owner);

    // Unlike name or categories, the country code is not locked by verification.
    let code = Some(String::from_str(&env, "GB"));
    client.set_country_code(&owner, &code);

    let ngo = client.get_ngo(&owner);
    assert!(ngo.verified);
    assert_eq!(ngo.country_code, code);
}

#[test]
fn get_ngo_returns_country_code() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let name = String::from_str(&env, "Example NGO");
    client.register(&owner, &name);

    let code = Some(String::from_str(&env, "US"));
    client.set_country_code(&owner, &code);

    let ngo = client.get_ngo(&owner);
    assert_eq!(ngo.country_code, code);
}

#[test]
fn set_country_code_all_valid_two_letter_uppercase_accepted() {
    // Spot-check a handful of real ISO 3166-1 alpha-2 codes.
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Example NGO"));

    for code_str in ["US", "GB", "DE", "FR", "ZA", "JP", "AU", "BR", "IN", "CN"] {
        let code = Some(String::from_str(&env, code_str));
        client.set_country_code(&owner, &code);
        assert_eq!(client.get_ngo(&owner).country_code, code);
    }
}
