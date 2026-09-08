//! Fixed basis-point waterfall: admin configures a split list that sums to
//! exactly 10_000, and the oracle triggers distributions that credit each
//! recipient's pull-payment balance using integer basis-point math.

use soroban_sdk::{Address, Env, Symbol, Vec};

use crate::errors::SplitStreamError;
use crate::storage;

/// Total basis points in a whole (100.00%).
const BASIS_POINTS: i128 = 10_000;

/// Validate and store the fixed share list. The basis-point values must sum
/// to exactly 10_000.
pub(crate) fn configure_fixed_shares(
    env: &Env,
    shares: &Vec<(Address, u32)>,
) -> Result<(), SplitStreamError> {
    let admin = storage::get_admin(env)?;
    admin.require_auth();
    storage::bump_instance(env);

    let mut sum: u64 = 0;
    for (_address, bps) in shares.iter() {
        sum += u64::from(bps);
    }
    if sum != 10_000 {
        return Err(SplitStreamError::InvalidShareTotal);
    }

    storage::set_fixed_shares(env, shares);
    env.events()
        .publish((Symbol::new(env, "fixed_shares_configured"),), shares.clone());
    Ok(())
}

/// Credit each fixed-share recipient `amount * bps / 10_000` (integer math,
/// no floats) to their claimable balance.
///
/// Auth: the oracle (the cycle-automated trigger). Note on SDK 27: the
/// `current_contract_invoker` caller-introspection API was removed in the 22.x
/// auth rework, and a failed `require_auth` aborts the whole invocation, so an
/// "admin OR oracle" check cannot be expressed in a single function. The
/// oracle is the relay the splitstream-actions daemon calls; the admin
/// exercises every other governance surface (shares, sweeps, challenges).
pub(crate) fn distribute_fixed(env: &Env, amount: i128) -> Result<(), SplitStreamError> {
    let oracle = storage::get_oracle(env)?;
    oracle.require_auth();

    if amount <= 0 {
        return Err(SplitStreamError::InvalidAmount);
    }

    let shares = storage::get_fixed_shares(env);
    if shares.is_empty() {
        // Nothing configured yet — nothing to distribute, no state changed.
        return Ok(());
    }

    for (address, bps) in shares.iter() {
        let share = amount
            .checked_mul(i128::from(bps))
            .and_then(|v| v.checked_div(BASIS_POINTS))
            .ok_or(SplitStreamError::InvalidAmount)?;
        storage::add_balance(env, &address, share)?;
        env.events().publish(
            (Symbol::new(env, "fixed_distributed"), address.clone(), share),
            (),
        );
    }
    Ok(())
}