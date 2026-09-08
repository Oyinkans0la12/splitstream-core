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

mod claims;
mod errors;
mod merkle;
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
    /// One-time initialization. Stores the admin, oracle (CI relay) and the
    /// SEP-41 token in instance storage.
    pub fn initialize(
        env: Env,
        admin: Address,
        oracle: Address,
        token: Address,
    ) -> Result<(), SplitStreamError> {
        admin.require_auth();
        if storage::is_initialized(&env) {
            return Err(SplitStreamError::AlreadyInitialized);
        }
        storage::set_admin(&env, &admin);
        storage::set_oracle(&env, &oracle);
        storage::set_token(&env, &token);
        storage::bump_instance(&env);

        env.events().publish(
            (
                Symbol::new(&env, "initialized"),
                admin.clone(),
                oracle.clone(),
                token.clone(),
            ),
            (),
        );
        Ok(())
    }

    /// Deposit pooled funds into the vault from `from` via the token contract.
    pub fn deposit(env: Env, from: Address, amount: i128) -> Result<(), SplitStreamError> {
        let token = storage::get_token(&env)?;
        from.require_auth();
        if amount <= 0 {
            return Err(SplitStreamError::InvalidAmount);
        }

        let contract = env.current_contract_address();
        let to: MuxedAddress = (&contract).into();
        token::TokenClient::new(&env, &token).transfer(&from, &to, &amount);

        env.events()
            .publish((Symbol::new(&env, "deposit"), from.clone(), amount), ());
        Ok(())
    }

    /// Post a cycle's payout-manifest root (oracle-gated). One post per cycle;
    /// corrections go through `challenge_and_replace_root`.
    pub fn post_cycle_root(
        env: Env,
        cycle_id: u64,
        root: BytesN<32>,
        total_amount: i128,
    ) -> Result<(), SplitStreamError> {
        let oracle = storage::get_oracle(&env)?;
        oracle.require_auth();

        if storage::get_cycle_info(&env, cycle_id).is_some() {
            return Err(SplitStreamError::CycleAlreadyPosted);
        }

        let info = types::CycleInfo {
            root: root.clone(),
            total_amount,
            posted_at: env.ledger().timestamp(),
            claims_started: false,
            replaced: false,
        };
        storage::set_cycle_info(&env, cycle_id, &info);

        env.events().publish(
            (
                Symbol::new(&env, "cycle_posted"),
                cycle_id,
                root.clone(),
                total_amount,
            ),
            (),
        );
        Ok(())
    }

    /// Challenge and replace a cycle's root within the dispute window
    /// (admin-gated — deliberately not the oracle, so the automated relay
    /// cannot override its own dispute window).
    ///
    /// Fails once claims have started, once the 24h window has closed, or on a
    /// second replacement. The original `posted_at` is preserved: the window
    /// bounds total dispute time to 24h from the original post.
    pub fn challenge_and_replace_root(
        env: Env,
        cycle_id: u64,
        new_root: BytesN<32>,
        new_total_amount: i128,
    ) -> Result<(), SplitStreamError> {
        let admin = storage::get_admin(&env)?;
        admin.require_auth();
        storage::bump_instance(&env);

        let mut cycle =
            storage::get_cycle_info(&env, cycle_id).ok_or(SplitStreamError::CycleNotFound)?;

        if cycle.claims_started {
            return Err(SplitStreamError::ClaimsAlreadyStarted);
        }
        let elapsed = env.ledger().timestamp().saturating_sub(cycle.posted_at);
        if elapsed >= types::CHALLENGE_WINDOW_SECS {
            return Err(SplitStreamError::ChallengeWindowClosed);
        }
        if cycle.replaced {
            return Err(SplitStreamError::RootAlreadyReplaced);
        }

        cycle.root = new_root.clone();
        cycle.total_amount = new_total_amount;
        cycle.replaced = true;
        storage::set_cycle_info(&env, cycle_id, &cycle);

        env.events().publish(
            (
                Symbol::new(&env, "cycle_root_replaced"),
                cycle_id,
                new_root.clone(),
                new_total_amount,
            ),
            (),
        );
        Ok(())
    }

    /// Prove a `(contributor, amount)` entry against the current cycle root
    /// and credit the contributor's pull-payment balance.
    ///
    /// Enforces the challenge window: claims are rejected until 24h have
    /// elapsed since `posted_at`. The first successful claim sets
    /// `claims_started`, making the root final.
    pub fn credit_claim(
        env: Env,
        contributor: Address,
        cycle_id: u64,
        amount: i128,
        proof: Vec<BytesN<32>>,
    ) -> Result<(), SplitStreamError> {
        claims::credit_claim(&env, &contributor, cycle_id, amount, &proof)
    }
}