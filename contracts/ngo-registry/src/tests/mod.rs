// SPDX-License-Identifier: Apache-2.0
//! Tests for ngo-registry, split by feature area.
//!
//! | Module       | Coverage                                                             |
//! |--------------|----------------------------------------------------------------------|
//! | helpers      | Shared fixtures: `setup`, event, auth and TTL assertions, name builders |
//! | registration | register/unregister, names and uniqueness, `owner_by_name`, metadata URI, categories, `touch_ngo`, owner transfer |
//! | verification | approve/batch_approve/revoke/reject, approver permissions, expiry and renewal, `verified_count`, `verification_status` |
//! | admin        | Initialisation, upgrades and timelock, registration pause, approver list, verification period, `remove_ngo`, roles doc |
//! | paging       | `list_ngos`, `list_ngos_page`, NGO count and `total_ngos`             |

mod helpers;

mod admin;
mod paging;
mod registration;
mod verification;
