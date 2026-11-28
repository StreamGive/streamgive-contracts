// SPDX-License-Identifier: Apache-2.0
#![no_std]
use soroban_sdk::{contracttype, Address, String};

/// A registered NGO record, shared between the ngo-registry and donation-vault
/// contracts. Keeping it in one place prevents the two contracts from drifting
/// apart and causing silent decode failures during cross-contract calls.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Ngo {
    pub owner: Address,
    pub name: String,
    pub verified: bool,
    /// Optional owner-supplied reference to off-chain metadata (a website,
    /// description, or logo). `None` until the owner calls `set_metadata_uri`.
    pub metadata_uri: Option<String>,
    /// Optional ledger timestamp at which this verification lapses. `None`
    /// means the verification never expires.
    pub verified_until: Option<u64>,
}
