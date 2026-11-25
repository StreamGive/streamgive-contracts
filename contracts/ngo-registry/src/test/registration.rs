// SPDX-License-Identifier: Apache-2.0

use super::*;
use soroban_sdk::testutils::{AuthorizedFunction, Events as _, MockAuth, MockAuthInvoke};
use soroban_sdk::{symbol_short, vec, IntoVal, Symbol, Val, Vec};

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
fn register_bumps_instance_and_ngo_ttl() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    client.register(&owner, &String::from_str(&env, "Red Cross"));

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

// =============================================================================
// register and unregister auth and events (issue #294)
// =============================================================================

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
// NGO owner transfer tests (keeping verified flag)
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
