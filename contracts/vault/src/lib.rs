//! # SplitStreamVault
//!
//! A Soroban contract that pools funds (a single SEP-41 token per deployment)
//! and settles them to contributors via three strategies:
//!
//! 1. **Merkle-proof claims** — Wave-cycle points payouts. A cycle root is
//!    posted by the oracle and enters a 24h challenge window during which the
//!    admin may replace it (once) before any claim can extract funds against
//!    it. After the window, contributors claim by proving a `(contributor,
//!    amount)` leaf against the stored root and are credited a pull-payment
//!    balance they withdraw themselves.
//! 2. **Fixed basis-point waterfalls** — recurring team/community/reserve
//!    splits, configured by the admin to sum to 10_000 bps and distributed by
//!    the oracle.
//! 3. **Linear vesting streams** — maintainer retention rewards, paid directly
//!    on claim.
//!
//! The contract never loops over an unbounded contributor list in a single
//! transaction (the fixed-split loop is bounded by a small, admin-configured
//! list).

#![no_std]
// Event topic names are frozen by the splitstream spec (e.g. "claim_credited",
// "cycle_root_replaced"); they exceed symbol_short!'s 9-char limit, so they are
// built with full `Symbol`s. `Events::publish` is deprecated in favor of
// `#[contractevent]` but remains fully supported in SDK 27.
#![allow(deprecated)]
#[cfg(test)]
extern crate std;

mod errors;
mod storage;
mod types;

use soroban_sdk::{contract, contractimpl, token, Address, BytesN, Env, MuxedAddress, Symbol, Vec};

pub use crate::errors::SplitStreamError;
pub use crate::types::{CycleInfo, SweepRequestData, VestingData};

/// The SplitStream payout vault.
#[contract]
pub struct SplitStreamVault;

#[contractimpl]
impl SplitStreamVault {
}