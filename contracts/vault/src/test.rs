//! Test suites for SplitStreamVault.
//!
//! One module per feature: deposit/pull-payment claims, Merkle verification,
//! challenge-window semantics, fixed-split/vesting/sweep timelocks, and
//! per-address authorization.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke},
    token, vec, Address, BytesN, Env, IntoVal, Symbol, Val, Vec,
};

use crate::errors::SplitStreamError;
use crate::merkle::{hash_pair, leaf_hash, verify_proof};
use crate::sweep::SWEEP_TIMELOCK_SECS;
use crate::types::CHALLENGE_WINDOW_SECS;
use crate::{SplitStreamVault, SplitStreamVaultClient};

/// Test fixture: an initialized vault backed by a mintable SAC.
///
/// Clients are constructed on demand (they borrow the `Env`), so the fixture
/// only holds addresses.
struct TestEnv {
    env: Env,
    admin: Address,
    oracle: Address,
    token: Address,
    contract_id: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract_v2(admin.clone());
    let token = token_contract.address();
    let contract_id = env.register(SplitStreamVault, ());
    SplitStreamVaultClient::new(&env, &contract_id).initialize(&admin, &oracle, &token);
    TestEnv {
        env,
        admin,
        oracle,
        token,
        contract_id,
    }
}

impl TestEnv {
    fn client(&self) -> SplitStreamVaultClient<'_> {
        SplitStreamVaultClient::new(&self.env, &self.contract_id)
    }

    fn sac(&self) -> token::StellarAssetClient<'_> {
        token::StellarAssetClient::new(&self.env, &self.token)
    }

    fn token_balance(&self, addr: &Address) -> i128 {
        token::TokenClient::new(&self.env, &self.token).balance(addr)
    }
}

/// Build a balanced Merkle tree over manifest entries using the same
/// sorted-pair convention as the verifier, returning the root and per-entry
/// bottom-up proofs. `entries` must be a power-of-two count.
fn build_manifest(
    env: &Env,
    entries: &[(Address, i128)],
) -> (BytesN<32>, std::vec::Vec<(Address, i128, Vec<BytesN<32>>)>) {
    let mut leaves: std::vec::Vec<(Address, i128, BytesN<32>)> = entries
        .iter()
        .map(|(addr, amt)| (addr.clone(), *amt, leaf_hash(env, addr, *amt)))
        .collect();
    leaves.sort_by(|a, b| a.2.cmp(&b.2));
    let n = leaves.len();
    assert!(n.is_power_of_two(), "test manifests use power-of-two leaves");

    // Heap-style tree array: unused index 0, internal nodes at [1, n),
    // leaves at [n, 2n).
    let mut tree: std::vec::Vec<BytesN<32>> = std::vec::Vec::with_capacity(2 * n);
    tree.push(leaf_hash(env, &Address::generate(env), 0)); // unused index 0
    for _ in 1..n {
        tree.push(leaf_hash(env, &Address::generate(env), 0)); // internal [1, n)
    }
    for leaf in &leaves {
        tree.push(leaf.2.clone());
    }
    for i in (1..n).rev() {
        let (a, b) = (tree[2 * i].clone(), tree[2 * i + 1].clone());
        tree[i] = hash_pair(env, &a, &b);
    }
    let root = tree[1].clone();

    let mut result = std::vec::Vec::new();
    for (idx, (addr, amt, _)) in leaves.iter().enumerate() {
        let mut proof = std::vec::Vec::new();
        let mut node = n + idx;
        while node > 1 {
            let sibling = if node % 2 == 0 {
                tree[node + 1].clone()
            } else {
                tree[node - 1].clone()
            };
            proof.push(sibling);
            node /= 2;
        }
        let mut proof_vec = Vec::new(env);
        for p in &proof {
            proof_vec.push_back(p.clone());
        }
        result.push((addr.clone(), *amt, proof_vec));
    }
    (root, result)
}

/// Look up the proof for a specific contributor.
fn proof_for<'a>(
    claims: &'a [(Address, i128, Vec<BytesN<32>>)],
    addr: &Address,
) -> &'a Vec<BytesN<32>> {
    claims
        .iter()
        .find(|(a, _, _)| a == addr)
        .map(|(_, _, p)| p)
        .expect("contributor present in manifest")
}

/// Mint tokens and deposit them into the vault.
fn fund_vault(t: &TestEnv, amount: i128) {
    t.sac().mint(&t.admin, &amount);
    t.client().deposit(&t.admin, &amount);
}
mod deposit_claims {
    use super::*;

    #[test]
    fn deposit_transfers_tokens_and_emits_event() {
        let t = setup();
        t.sac().mint(&t.admin, &10_000);
        t.client().deposit(&t.admin, &1_000);

        // Read events before any further invocation, since `events().all()`
        // only reports the last contract call, and filter to this contract
        // (the token's inner `transfer` event is also emitted).
        assert_eq!(
            t.env.events().all().filter_by_contract(&t.contract_id),
            vec![
                &t.env,
                (
                    t.contract_id.clone(),
                    (Symbol::new(&t.env, "deposit"), t.admin.clone(), 1_000_i128)
                        .into_val(&t.env),
                    ().into_val(&t.env),
                )
            ]
        );

        assert_eq!(t.token_balance(&t.contract_id), 1_000);
    }

    #[test]
    fn deposit_rejects_zero_or_negative_amounts() {
        let t = setup();
        assert_eq!(
            t.client().try_deposit(&t.admin, &0),
            Err(Ok(SplitStreamError::InvalidAmount))
        );
        assert_eq!(
            t.client().try_deposit(&t.admin, &-5),
            Err(Ok(SplitStreamError::InvalidAmount))
        );
    }

    #[test]
    fn initialize_rejects_second_call() {
        let t = setup();
        assert_eq!(
            t.client().try_initialize(&t.admin, &t.oracle, &t.token),
            Err(Ok(SplitStreamError::AlreadyInitialized))
        );
    }

    #[test]
    fn claim_credits_balance_and_withdraw_pays_out() {
        let t = setup();
        fund_vault(&t, 10_000);

        let (root, claims) =
            build_manifest(&t.env, &[(t.admin.clone(), 400), (t.oracle.clone(), 600)]);
        t.client().post_cycle_root(&1, &root, &1_000);
        t.env.ledger().set_timestamp(CHALLENGE_WINDOW_SECS);

        t.client()
            .credit_claim(&t.admin, &1, &400, proof_for(&claims, &t.admin));
        assert_eq!(t.client().get_balance(&t.admin), 400);

        t.client().withdraw(&t.admin);
        assert_eq!(t.client().get_balance(&t.admin), 0);
        assert_eq!(t.token_balance(&t.admin), 400);
    }

    #[test]
    fn withdraw_rejects_empty_balance() {
        let t = setup();
        assert_eq!(
            t.client().try_withdraw(&t.admin),
            Err(Ok(SplitStreamError::InsufficientBalance))
        );
    }

    #[test]
    fn claim_rejects_double_claim() {
        let t = setup();
        let (root, claims) = build_manifest(&t.env, &[(t.admin.clone(), 100)]);
        t.client().post_cycle_root(&1, &root, &100);
        t.env.ledger().set_timestamp(CHALLENGE_WINDOW_SECS);
        t.client()
            .credit_claim(&t.admin, &1, &100, proof_for(&claims, &t.admin));
        assert_eq!(
            t.client()
                .try_credit_claim(&t.admin, &1, &100, proof_for(&claims, &t.admin)),
            Err(Ok(SplitStreamError::AlreadyClaimed))
        );
        assert!(t.client().has_claimed(&1, &t.admin));
    }

    #[test]
    fn claim_rejects_unknown_cycle_and_bad_proof() {
        let t = setup();
        let (root, claims) = build_manifest(&t.env, &[(t.admin.clone(), 100)]);
        t.client().post_cycle_root(&1, &root, &100);
        t.env.ledger().set_timestamp(CHALLENGE_WINDOW_SECS);

        assert_eq!(
            t.client()
                .try_credit_claim(&t.admin, &99, &100, proof_for(&claims, &t.admin)),
            Err(Ok(SplitStreamError::CycleNotFound))
        );
        let wrong = vec![&t.env, BytesN::from_array(&t.env, &[0xAB; 32])];
        assert_eq!(
            t.client().try_credit_claim(&t.admin, &1, &100, &wrong),
            Err(Ok(SplitStreamError::InvalidProof))
        );
        assert_eq!(
            t.client()
                .try_credit_claim(&t.admin, &1, &101, proof_for(&claims, &t.admin)),
            Err(Ok(SplitStreamError::InvalidProof))
        );
    }
}
