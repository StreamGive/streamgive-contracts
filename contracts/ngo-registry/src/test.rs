// SPDX-License-Identifier: Apache-2.0
#![cfg(test)]

use super::*;
use soroban_sdk::testutils::storage::{Instance as _, Persistent as _};
use soroban_sdk::testutils::{
    Address as _, AuthorizedFunction, Events as _, Ledger, MockAuth, MockAuthInvoke,
};
use soroban_sdk::{symbol_short, vec, IntoVal, Symbol, Val, Vec};

mod admin;
mod common;
mod paging;
mod registration;
mod verification;
