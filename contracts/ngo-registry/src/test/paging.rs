// SPDX-License-Identifier: Apache-2.0
use super::common::*;
use super::*;

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
fn list_ngos_page_zero_limit_returns_empty_with_total() {
    let (env, client, _admin) = setup();
    for _ in 0..5 {
        let owner = Address::generate(&env);
        client.register(&owner, &String::from_str(&env, "NGO"));
    }

    let page = client.list_ngos_page(&0, &0);

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
