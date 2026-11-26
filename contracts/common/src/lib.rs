// SPDX-License-Identifier: Apache-2.0
//! Shared constants used by the StreamGive contracts.
#![no_std]

/// Approximate ledgers per day at a 5-second close time. Used to express
/// storage TTLs (which the network counts in ledgers, not wall time) in
/// human terms.
pub const DAY_IN_LEDGERS: u32 = 17_280;

/// Instance storage bump amount (30 days in ledgers).
pub const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;

/// Instance storage lifetime threshold (29 days in ledgers).
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Persistent entry bump amount (90 days in ledgers).
pub const PERSISTENT_BUMP_AMOUNT: u32 = 90 * DAY_IN_LEDGERS;

/// Persistent entry lifetime threshold (89 days in ledgers).
pub const PERSISTENT_LIFETIME_THRESHOLD: u32 = PERSISTENT_BUMP_AMOUNT - DAY_IN_LEDGERS;
