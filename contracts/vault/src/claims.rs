//! Pull-payment claim path: `credit_claim` credits a claimable balance after
//! Merkle-proof and challenge-window checks; `withdraw` pays it out.

use soroban_sdk::{Address, BytesN, Env, MuxedAddress, Symbol, Vec};

use crate::errors::SplitStreamError;
use crate::merkle;
use crate::storage;
use crate::types::CHALLENGE_WINDOW_SECS;

/// Credit a contributor's claimable balance for a cycle.
///
/// Enforcement order: cycle exists → challenge window elapsed → not already
/// claimed → proof verifies against the currently stored root (post-replacement
/// if one occurred). The first successful claim locks the root in place.
pub(crate) fn credit_claim(
    env: &Env,
    contributor: &Address,
    cycle_id: u64,
    amount: i128,
    proof: &Vec<BytesN<32>>,
) -> Result<(), SplitStreamError> {
    contributor.require_auth();

    let mut cycle = storage::get_cycle_info(env, cycle_id).ok_or(SplitStreamError::CycleNotFound)?;

    if amount <= 0 {
        return Err(SplitStreamError::InvalidAmount);
    }

    // Enforcement point for the dispute window — a posted root is not
    // claimable until the full challenge window has elapsed.
    let elapsed = env
        .ledger()
        .timestamp()
        .saturating_sub(cycle.posted_at);
    if elapsed < CHALLENGE_WINDOW_SECS {
        return Err(SplitStreamError::ClaimsNotYetOpen);
    }

    if storage::get_cycle_claimed(env, cycle_id, contributor) {
        return Err(SplitStreamError::AlreadyClaimed);
    }

    let leaf = merkle::leaf_hash(env, contributor, amount);
    if !merkle::verify_proof(env, &leaf, proof, &cycle.root) {
        return Err(SplitStreamError::InvalidProof);
    }

    // Success: lock the root, mark claimed, credit the balance.
    cycle.claims_started = true;
    storage::set_cycle_info(env, cycle_id, &cycle);
    storage::set_cycle_claimed(env, cycle_id, contributor);
    storage::add_balance(env, contributor, amount)?;

    env.events().publish(
        (
            Symbol::new(env, "claim_credited"),
            contributor.clone(),
            cycle_id,
            amount,
        ),
        (),
    );
    Ok(())
}
