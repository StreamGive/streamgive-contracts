// SPDX-License-Identifier: Apache-2.0
//! Tests for enumerating the registry: `list_ngos`, `list_ngos_page`, and the
//! NGO count behind `ngo_count` / `total_ngos`.

use super::helpers::*;
use crate::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::Vec;

// ── list_ngos ─────────────────────────────────────────────────────────────────

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
    for i in 0..5 {
        let owner = Address::generate(&env);
        let name = numbered_name(&env, i);
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
    for i in 0..55 {
        let owner = Address::generate(&env);
        client.register(&owner, &numbered_name(&env, i));
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

#[test]
fn list_ngos_handles_removed_entries_correctly() {
    let (env, client, _admin) = setup();

    let mut owners: Vec<Address> = Vec::new(&env);
    for name in ["NGO 0", "NGO 1", "NGO 2", "NGO 3", "NGO 4"] {
        let owner = Address::generate(&env);
        client.register(&owner, &String::from_str(&env, name));
        owners.push_back(owner);
    }
    assert_eq!(client.ngo_count(), 5);

    let initial_page = client.list_ngos_page(&0, &10);
    assert_eq!(initial_page.owners.len(), 5);
    assert_eq!(initial_page.next, 5);

    // Remove the NGO from the middle of the index.
    let middle_owner = owners.get(2).unwrap();
    client.remove_ngo(&middle_owner);
    assert_eq!(client.ngo_count(), 4);

    let after_removal_page = client.list_ngos_page(&0, &10);
    assert_eq!(after_removal_page.owners.len(), 4);
    assert_eq!(after_removal_page.next, 4);
    assert!(!after_removal_page.owners.contains(&middle_owner));

    // Every other NGO is still accessible; the removed one is not.
    for (i, owner) in owners.iter().enumerate() {
        if i == 2 {
            assert_eq!(client.try_get_ngo(&owner), Err(Ok(Error::NotRegistered)));
        } else {
            assert!(client.try_get_ngo(&owner).is_ok());
        }
    }

    // Paging across the removed slot yields each remaining NGO exactly once.
    let first_page = client.list_ngos_page(&0, &2);
    assert_eq!(first_page.owners.len(), 2);
    assert_eq!(first_page.next, 2);

    let second_page = client.list_ngos_page(&2, &2);
    assert_eq!(second_page.owners.len(), 2);
    assert_eq!(second_page.next, 4);

    let mut all_owners_after = first_page.owners.clone();
    all_owners_after.append(&second_page.owners);
    assert_eq!(all_owners_after.len(), 4);
    assert!(!all_owners_after.contains(&middle_owner));
    for (i, owner) in owners.iter().enumerate() {
        if i != 2 {
            assert!(all_owners_after.contains(&owner));
        }
    }
}

// ── list_ngos_page ────────────────────────────────────────────────────────────

#[test]
fn list_ngos_page_first_page_returns_next_start() {
    let (env, client, _admin) = setup();
    let mut owners = soroban_sdk::vec![&env];
    for i in 0..5 {
        let owner = Address::generate(&env);
        client.register(&owner, &numbered_name(&env, i));
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
    for i in 0..5 {
        let owner = Address::generate(&env);
        client.register(&owner, &numbered_name(&env, i));
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
    for i in 0..4 {
        let owner = Address::generate(&env);
        client.register(&owner, &numbered_name(&env, i));
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
    for i in 0..7 {
        let owner = Address::generate(&env);
        client.register(&owner, &numbered_name(&env, i));
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

// ── NGO count ─────────────────────────────────────────────────────────────────

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
