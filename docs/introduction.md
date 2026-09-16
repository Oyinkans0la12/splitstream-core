# Introduction

SplitStream is a Soroban settlement protocol that lets a Drips Wave team turn
pooled treasury funds into a verifiable, disputable, on-chain distribution to
contributors, instead of manual off-chain calculation. It spans three repos:
[splitstream-actions](https://splitstream.gitbook.io/splitstream-actions/)
(the GitHub→chain bridge that computes each cycle's payout manifest and relays
its Merkle root on-chain),
[splitstream-sdk-cli](https://splitstream.gitbook.io/splitstream-sdk-cli/)
(the client SDK and CLI contributors claim with), and this repo,
**splitstream-core**.

## This repo's role

splitstream-core is the on-chain settlement layer. It holds the funds and
enforces the rules: Merkle-proof claims, fixed basis-point splits, linear
vesting, and the challenge window. It does not know about GitHub, points, or
issue counts. It only ever verifies `(address, amount)` pairs and pays them.

Everything that decides *what* a contributor is owed happens elsewhere. This
contract decides only whether a claimed amount is provable against the root
that was posted, and whether the rules allow the payout to happen yet.

## The problem it solves

Wave payouts are usually settled off-chain. That produces three failures:

- **Squabbling over splits.** Points, weights, and shares are argued over in
  chat, with no authoritative artifact to point at.
- **Delayed disbursement.** Someone has to compute and send each payment by
  hand, so contributors wait.
- **No audit trail.** There is no on-chain record binding an amount to a
  contributor to a specific cycle.

This contract fixes all three by making the distribution an on-chain fact: a
committed Merkle root, a fixed window in which maintainers can correct it, and
per-contributor claims that anyone can verify against the chain.
