//! Linear vesting streams for maintainer retention rewards. Claims pay out
//! immediately from the contract's token balance (a separate accounting track
//! from the `Balance` pull-payment balances).

use soroban_sdk::{Address, Env, MuxedAddress, Symbol};

use crate::errors::SplitStreamError;
use crate::storage;
use crate::types::VestingData;

/// Create (or re-create) a linear vesting schedule for a contributor.
///
/// Re-creating a schedule preserves `claimed` from any existing schedule so a
/// re-created schedule can never let a contributor re-claim already-vested
/// funds; only the total, start and duration change.
pub(crate) fn create_vesting(
    env: &Env,
    contributor: &Address,
    total: i128,
    duration_ledgers: u32,
) -> Result<(), SplitStreamError> {
    let admin = storage::get_admin(env)?;
    admin.require_auth();
    storage::bump_instance(env);

    if total <= 0 || duration_ledgers == 0 {
        return Err(SplitStreamError::InvalidAmount);
    }

    let existing_claimed = storage::get_vesting(env, contributor)
        .map(|v| v.claimed)
        .unwrap_or(0);

    let schedule = VestingData {
        total,
        claimed: existing_claimed,
        start_ledger: env.ledger().sequence(),
        duration_ledgers,
    };
    storage::set_vesting(env, contributor, &schedule);

    env.events().publish(
        (
            Symbol::new(env, "vesting_created"),
            contributor.clone(),
            total,
            duration_ledgers,
        ),
        (),
    );
    Ok(())
}

/// Claim the newly vested amount and transfer it directly to the contributor.
///
/// Returns the amount claimed; `0` is a legitimate "nothing new yet" state and
/// is not an error.
pub(crate) fn claim_vested(env: &Env, contributor: &Address) -> Result<i128, SplitStreamError> {
    contributor.require_auth();

    let token = storage::get_token(env)?;
    // The frozen error enum has no "vesting not found" variant; InsufficientBalance
    // is the closest semantic ("there is nothing claimable"). Tracked as a
    // Phase-13 follow-up (add a dedicated error variant).
    let mut schedule = storage::get_vesting(env, contributor)
        .ok_or(SplitStreamError::InsufficientBalance)?;

    let elapsed = env
        .ledger()
        .sequence()
        .saturating_sub(schedule.start_ledger)
        .min(schedule.duration_ledgers);

    let vested = schedule
        .total
        .checked_mul(i128::from(elapsed))
        .and_then(|v| v.checked_div(i128::from(schedule.duration_ledgers)))
        .ok_or(SplitStreamError::InvalidAmount)?;

    let claimable = vested - schedule.claimed;
    if claimable <= 0 {
        return Ok(0);
    }

    schedule.claimed += claimable;
    storage::set_vesting(env, contributor, &schedule);

    let contract = env.current_contract_address();
    let to: MuxedAddress = contributor.into();
    soroban_sdk::token::TokenClient::new(env, &token).transfer(&contract, &to, &claimable);

    env.events().publish(
        (Symbol::new(env, "vested_claimed"), contributor.clone(), claimable),
        (),
    );
    Ok(claimable)
}