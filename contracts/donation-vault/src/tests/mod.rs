// SPDX-License-Identifier: Apache-2.0
//! Tests for donation-vault, split by feature area.
//!
//! | Module      | Coverage                                                        |
//! |-------------|------------------------------------------------------------------|
//! | helpers     | Shared fixtures, event helpers, auth/TTL utilities              |
//! | streams     | Stream lifecycle, TTL, status, transfer, depletion, by_donor/ngo |
//! | fees        | Protocol fee, treasury, per-token overrides, payout address, surplus |
//! | admin       | Admin transfer, renounce, upgrade, timelock, registry, config   |
//! | pausing     | Vault pause/unpause, rescue_stream, stream pause/resume         |
//! | batch       | withdraw_batch, withdraw_all, batch error conditions            |

pub mod helpers;

mod admin;
mod batch;
mod fees;
mod pausing;
mod streams;
