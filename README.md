# splitstream-core

A Soroban smart contract, **SplitStreamVault**, that pools funds (a single
SEP-41 token per deployment — USDC or native XLM SAC) and settles them to
contributors via three strategies:

1. **Merkle-proof claims** — Wave-cycle points payouts. The oracle posts a
   payout-manifest root per cycle; after a 24-hour dispute window contributors
   claim by proving a `(contributor, amount)` leaf against the root and are
   credited a pull-payment balance they withdraw themselves.
2. **Fixed basis-point waterfalls** — recurring team/community/reserve splits.
   The admin configures a share list summing to exactly 10,000 bps and the
   oracle triggers distributions.
3. **Linear vesting streams** — maintainer retention rewards, paid directly on
   claim.

The contract is pull-payment throughout: it never loops over an unbounded
contributor list in a single transaction.

## The challenge-window mechanism

A posted cycle root is **not immediately claimable**. Maintainers have a fixed
24-hour window (`CHALLENGE_WINDOW_SECS = 86_400`) to catch a bad points
calculation and correct it on-chain, **once**, via
`challenge_and_replace_root` — before any contributor can extract funds against
it. Once the first claim succeeds, the root is final: replacement is rejected
after `claims_started` is set, and the window itself is bounded to 24h from the
original post (replacement never restarts it).

## Contract functions

| Function | Auth | Purpose |
|---|---|---|
| `initialize(admin, oracle, token)` | admin | One-time setup; stores admin/oracle/SEP-41 token. |
| `deposit(from, amount)` | from | Pulls `amount` of the token into the vault. |
| `post_cycle_root(cycle_id, root, total_amount)` | oracle | Posts a payout-manifest root; one post per cycle. |
| `challenge_and_replace_root(cycle_id, new_root, new_total_amount)` | admin | Replaces the root within the dispute window (once). |
| `credit_claim(contributor, cycle_id, amount, proof)` | contributor | Verifies the Merkle proof and credits the claimable balance. |
| `withdraw(contributor)` | contributor | Pays out the claimable balance (checks-effects-interactions). |
| `configure_fixed_shares(shares)` | admin | Sets the bps split list (must sum to 10_000). |
| `distribute_fixed(amount)` | oracle | Credits each recipient `amount * bps / 10_000`. |
| `admin_distribute_fixed(amount)` | admin | Manual distribution; same body/events as `distribute_fixed`. |
| `create_vesting(contributor, total, duration_ledgers)` | admin | Creates/re-creates a linear vesting schedule (preserves `claimed`). |
| `claim_vested(contributor)` | contributor | Claims newly vested tokens, transferring them directly. |
| `request_sweep(to, amount)` | admin | Registers a sweep request, starting the 72h timelock. |
| `execute_sweep()` | admin | Executes the sweep once the timelock has elapsed. |
| `cancel_sweep()` | admin | Clears a pending sweep request (idempotent). |
| `get_balance(contributor)` / `get_cycle_info(cycle_id)` / `has_claimed(cycle_id, contributor)` / `get_vesting(contributor)` / `get_fixed_shares()` | — | Read-only views. |

## Storage design

`Admin`, `Oracle`, `Token` live in **instance** storage (TTL bumped on every
admin-gated call). `CycleData`, `CycleClaimed`, `Balance`, `FixedShares`,
`Vesting` and `SweepRequest` live in **persistent** storage; every write
extends the entry TTL in the same call (`storage::bump_persistent`) — a missing
`extend_ttl` is the classic Soroban bug where accounting silently expires.

## Building and testing

Rust 1.84+ is required (the `wasm32v1-none` target). Pinned SDK:
`soroban-sdk = "27.0.6"` (latest stable; do not use release candidates).

```bash
# Tests (soroban-sdk testutils; no network needed)
cargo test --workspace

# Deployable wasm — build with stellar-cli, never `cargo build` for the crate
stellar contract build --package splitstream-vault
# artifact: target/wasm32v1-none/release/splitstream_vault.wasm (~20 KB)
```

## Design decisions & known deviations

These are deliberate, documented decisions (each noted in its commit message):

- **`SweepRequest` is persistent, not temporary.** Temporary entries have a
  Mainnet max TTL of 17,280 ledgers (~1 day), shorter than the 72-hour sweep
  timelock — a request there would expire before it could execute.
- **`distribute_fixed` is split into two auth-gated functions.** SDK 27
  removed `current_contract_invoker` (the caller-introspection API) in the
  22.x auth rework, and a failed `require_auth` aborts the invocation, so an
  "admin OR oracle" check cannot be expressed in one function. `distribute_fixed`
  is oracle-gated (the automated trigger); `admin_distribute_fixed` is the
  admin-gated manual twin. Both delegate to one shared internal body, so they
  can never drift.
- **`claim_vested` with no schedule returns `NoVestingSchedule`** (error 17) —
  a dedicated variant so downstream SDKs can distinguish "no schedule" from
  "no balance".
- **Event topic names are full `Symbol`s** (e.g. `"claim_credited"`,
  `"cycle_root_replaced"`) via the (deprecated-but-supported)
  `Events::publish` API, preserving the frozen topic names from the spec.

## Repository layout

```
contracts/vault/src/
├── lib.rs         # #[contract] + #[contractimpl] entry points
├── storage.rs     # DataKey enum + typed get/set helpers with TTL bumps
├── types.rs       # CycleInfo, VestingData, SweepRequestData
├── errors.rs      # #[contracterror] SplitStreamError
├── merkle.rs      # leaf hashing + sorted-pair proof verification
├── claims.rs      # credit_claim, withdraw
├── fixed_split.rs # configure_fixed_shares, distribute_fixed, admin_distribute_fixed
├── vesting.rs     # create_vesting, claim_vested
├── sweep.rs       # request/execute/cancel sweep
└── test.rs        # one test module per feature
```

See `CONTRIBUTING.md` for development conventions and `SECURITY.md` for the
security model and reporting process.