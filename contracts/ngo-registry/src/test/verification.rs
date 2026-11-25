// SPDX-License-Identifier: Apache-2.0
use super::common::*;
use super::*;

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

    // No public entry point removes a verified NGO â€” unregister, remove_ngo and
    // reject_ngo all refuse it â€” so drive the shared delete helper directly to
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
