# Bousol Contracts

Soroban smart contracts for the **BOUSOL** project — a decentralized ROSCA (Rotating Savings and Credit Association) on the Stellar network.

---

## Project Structure

```text
.
├── contracts/
│   ├── rosca/          # V1 — simple admin-controlled transfer contract (legacy)
│   └── rosca_v2/       # V2 — full ROSCA implementation
│       ├── src/
│       │   ├── lib.rs       # Contract entry points
│       │   ├── types.rs     # Data structures & member logic
│       │   ├── errors.rs    # Error codes
│       │   └── storage.rs   # Storage key definitions
│       └── test_snapshots/  # Soroban ledger snapshots for regression tests
├── ROSCA_V2_DESIGN.md  # Detailed design specification
├── Cargo.toml
└── README.md
```

---

## V1 — `RoscaContract` (Legacy)

A minimal contract. The admin initializes the contract, and can transfer tokens from the contract balance to any address. No ROSCA logic — superseded by V2.

---

## V2 — `RoscaV2Contract`

A fully decentralized ROSCA with deposit protection, insurance pool, progressive penalties, weighted-random payout selection, and on-chain governance.

### Core Concepts

#### 1. Dual Tracking System

Every member is tracked on two dimensions simultaneously:

| Dimension | Metric | Purpose |
|-----------|--------|---------|
| **Debt system** (amount) | `net_balance = total_contributed - total_received` | Exit condition — must be ≥ 0 to leave |
| **Points system** (count) | `priority_score = contribution_count - (receive_count × members) - violation_penalty` | Payout eligibility & selection weight |

A member can receive a payout only when **both** conditions are satisfied:
- `net_balance >= 0`
- `priority_score > 0`

#### 2. Deposit & Insurance Pool

**Deposit** acts as a security bond (not a prepayment):

```
min_deposit     = contribution_amount × 3
recommended     = contribution_amount × 5
max_deposit     = contribution_amount × 10
```

Deposits are returned in full on a clean exit. They are deducted progressively on violations.

**Insurance pool** is funded by:
- 2% of every contribution (insurance fee)
- Late fees from delayed contributions
- Forfeited deposits from kicked members

#### 3. Observation Period

New members must contribute `observation_contributions` times before becoming eligible to receive a payout. This prevents join-and-withdraw abuse.

When `all_members_observation = true`, even the founding members must pass the observation period before the first payout.

#### 4. Progressive Violation Penalties

Three-strike system — penalties escalate with each violation:

| Violation # | Deposit Deducted | Points Penalty | Lockout |
|------------|-----------------|----------------|---------|
| 1st | 1,000 | −3 pts | 2 rounds |
| 2nd | 2,000 | −5 pts | 5 rounds |
| 3rd | all remaining | — | **Kicked** |

When a member violates, the payout recipient is compensated through three layers in order:

```
1. Violator's deducted deposit  →  covers the shortfall first
2. Insurance pool               →  covers remaining gap (up to max_insurance_coverage)
3. Beneficiary bears the rest   →  capped at max_beneficiary_loss_rate (e.g. 10%)
```

#### 5. Progressive Late Fees

Members who miss the deadline but pay within the grace period incur a late fee instead of a violation:

| Late count | Fee rate |
|-----------|---------|
| 1st late | 5% |
| 2nd late | 10% |
| 3rd late | 20% |
| 4th late | treated as violation |

Members with a long on-time streak get a discount:
- ≥ 10 consecutive on-time contributions → 50% off the late fee
- ≥ 20 consecutive on-time contributions → 80% off the late fee

Late fees go directly into the insurance pool.

#### 6. Cooldown Period

After receiving a payout, a member enters a cooldown and cannot receive again until it expires. Three modes are supported:

```rust
CooldownType::FixedRounds(n)   // fixed number of rounds
CooldownType::DynamicMembers   // cooldown = member count at time of receipt
CooldownType::TimeBased(secs)  // time-based
```

#### 7. Weighted Random Payout Selection

Each round, `settle_round()` selects the recipient using on-chain PRNG (`env.prng()`):

1. Filter all members who satisfy `can_receive()` (active, net_balance ≥ 0, priority > 0, observation done, not in cooldown or lockout)
2. Assign each candidate a weight = `priority_score` (guaranteed > 0 by `can_receive`)
3. Weighted random draw using on-chain PRNG:
   ```
   random_value = env.prng().gen_range(0..total_weight)
   ```

Higher priority → higher probability, but not guaranteed. This gives lower-priority members a non-zero chance and prevents starvation.

`calculate_recipient()` is a read-only query that returns the highest-priority candidate without consuming randomness — useful for front-end display.

---

### `settle_round` — Permissionless Settlement

`settle_round` requires **no authentication**. Anyone can call it — a ROSCA member, a backend cron job, a third-party bot, or another contract — as long as the round period has ended.

**Built-in protections:**
- Time window: reverts if the current round has not ended yet
- Idempotent: the round counter advances after settlement, so double-calls are safe
- Uses on-chain PRNG for secure random recipient selection

This design eliminates the single point of failure from an admin-only settle function.

---

### On-Chain Governance

All major decisions are made via weighted voting. Voting weight = `contribution_count` (members who contributed more have more say).

Five proposal types:

| Proposal | Approval threshold | Voting period | Cooldown |
|----------|-------------------|---------------|----------|
| `EmergencyPayout` | > 66% | 48 hours | — |
| `UpdateConfig` | > 50% | 7 days | 7 days |
| `Dissolution(Normal)` | > 90% | 14 days | — |
| `Dissolution(Emergency)` | > 75% | 24 hours | — |
| `Pause` | > 66% | 48 hours | — |
| `Resume` | > 50% | 48 hours | — |

**Emergency Payout** lets a member request early access to their accrued net balance (requires ≥ 66% approval). After execution the member enters the normal cooldown period.

**Dissolution** refunds all members: deposit + positive net balance. The insurance pool surplus is split equally among active members.

**Pause/Resume** allows the group to temporarily halt rounds (e.g., during holidays). While paused, ExitPending members can claim refunds via `process_paused_exit()`.

Proposers may **cancel** their proposals before the voting period ends via `cancel_proposal()`.

---

### Contract API

#### Initialization
```rust
fn initialize(env: Env, admin: Address, config: RoscaConfig) -> Result<(), Error>
```

#### Member Management
```rust
fn join(env: Env, member: Address, deposit_amount: i128) -> Result<(), Error>
fn request_exit(env: Env, member: Address) -> Result<(), Error>
fn process_paused_exit(env: Env, member: Address) -> Result<(), Error>  // exit during Pause
fn sponsor(env: Env, sponsor: Address, candidate: Address) -> Result<(), Error>
fn top_up_deposit(env: Env, member: Address, amount: i128) -> Result<(), Error>
```

#### Contributions
```rust
fn contribute(env: Env, member: Address) -> Result<(), Error>
fn contribute_late(env: Env, member: Address) -> Result<(), Error>
```

#### Settlement
```rust
fn settle_round(env: Env) -> Result<(), Error>              // permissionless, uses on-chain PRNG
fn calculate_recipient(env: Env) -> Result<Option<Address>, Error>  // read-only
```

#### Governance
```rust
fn propose(env: Env, proposer: Address, proposal_type: ProposalType) -> Result<u64, Error>
fn vote(env: Env, voter: Address, proposal_id: u64, choice: VoteChoice) -> Result<(), Error>
fn cancel_proposal(env: Env, proposer: Address, proposal_id: u64) -> Result<(), Error>
fn execute_proposal(env: Env, executor: Address, proposal_id: u64) -> Result<(), Error>
```

#### Queries
```rust
fn get_config(env: Env) -> Result<RoscaConfig, Error>
fn get_member(env: Env, address: Address) -> Result<Member, Error>
fn get_members(env: Env) -> Result<Vec<Address>, Error>
fn get_round(env: Env, round_id: u64) -> Result<Round, Error>
fn get_insurance_pool(env: Env) -> Result<i128, Error>
fn get_statistics(env: Env) -> Result<Statistics, Error>
fn get_proposal(env: Env, proposal_id: u64) -> Result<Proposal, Error>
fn get_vote(env: Env, proposal_id: u64, voter: Address) -> Result<Vote, Error>
fn is_cancelled(env: Env, proposal_id: u64) -> bool
```

---

### Scenario Walkthrough (4-member ROSCA)

Config: 100 USDC/round, 7-day period, 150 USDC deposit, 2% insurance fee.

```
Round 1: All 4 contribute → Bob selected (random) → Bob receives 392 USDC
         Bob's net_balance: -292, enters cooldown until round 5

Round 2: All 4 contribute → Alice selected → Alice receives 392 USDC

Round 3: Carol violates (misses grace period)
         Carol's deposit: 150 → 100 (−50 deducted)
         Carol locked out until round 5
         Dave selected → receives 360 USDC (deposit covers 50, insurance covers 16, Dave bears 32)

Round 4: All contribute, Bob pays late fee (5%)
         No eligible recipient (all in cooldown/lockout) → contributions refunded

Round 5: Carol passes lockout → Dave selected again (priority score recovered)

... cycles continue until dissolution vote passes (>90%) or members exit individually
```

---

### Contract API (continued)

#### Admin
```rust
fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error>  // admin only
fn transfer_admin(env: Env, new_admin: Address) -> Result<(), Error>  // admin only, step 1
fn accept_admin(env: Env) -> Result<(), Error>                        // pending admin only, step 2
```

Admin transfer is two-step to prevent accidental loss of upgrade capability.

---

### Development

**Toolchain:**

| Component | Version |
|-----------|---------|
| Rust | 1.95+ (stable) |
| soroban-sdk | 25.3.1 |
| Stellar CLI | 25.2.0 |
| Build target | `wasm32v1-none` |

**Build:**
```bash
cargo build --target wasm32v1-none --release -p rosca_v2
```

**Run tests:**
```bash
cargo test --package rosca_v2
```

**Optimize WASM:**
```bash
stellar contract optimize \
  --wasm target/wasm32v1-none/release/rosca_v2.wasm
```

---

### Deployment

#### Mainnet (Production)

Deployed on **2026-04-28**.

| Item | Value |
|------|-------|
| Contract ID | `CCE7TPDTAZOFGU6ZCKDEBKBWWXKO27D4I5AHKYRXC2FUNATCPRFY72CI` |
| WASM Hash | `22e25896129abdba5d9d08d305788b0be910cd6f0b94f0843e82f75967e6329c` |
| Deployer | `bousol-pro` (`GAWMVAGTT3HZR3RA3MQIW7P6APJWOF7KC7FTQTQL3K2BS7P4JHVLOTYL`) |
| Admin | `rosca-admin` (`GB3K3HQG3D7RUURI3ICIEE3VQJHU6FCRC3HTW5MFYX2WKE6BGVO7PIA6`) |
| Network | Stellar Mainnet |
| Explorer | [stellar.expert](https://stellar.expert/explorer/public/contract/CCE7TPDTAZOFGU6ZCKDEBKBWWXKO27D4I5AHKYRXC2FUNATCPRFY72CI) |
| Deployment cost | ~58.6 XLM |

**Server environment variables:**
```
ROSCA_V2_CONTRACT_ID=CCE7TPDTAZOFGU6ZCKDEBKBWWXKO27D4I5AHKYRXC2FUNATCPRFY72CI
ROSCA_V2_ADMIN_ADDRESS=GB3K3HQG3D7RUURI3ICIEE3VQJHU6FCRC3HTW5MFYX2WKE6BGVO7PIA6
```

#### Account Roles

| Account | Alias | Purpose | Security |
|---------|-------|---------|----------|
| `GAWMVAGTT3...` | `bousol-pro` | Deployed the WASM | Medium (no ongoing role) |
| `GB3K3HQG3D...` | `rosca-admin` | Contract admin (upgrade only) | High (cold wallet, offline) |
| `FEE_ACCOUNT_SEED` | — | Server gas payer, settle_round caller | Medium (on server) |

#### Upgrade Procedure

```bash
# 1. Build and optimize new WASM
cargo build --target wasm32v1-none --release -p rosca_v2
stellar contract optimize --wasm target/wasm32v1-none/release/rosca_v2.wasm

# 2. Upload new WASM (get hash)
stellar contract install \
  --wasm target/wasm32v1-none/release/rosca_v2.optimized.wasm \
  --source rosca-admin \
  --network mainnet

# 3. Call upgrade with the new hash
stellar contract invoke \
  --id CCE7TPDTAZOFGU6ZCKDEBKBWWXKO27D4I5AHKYRXC2FUNATCPRFY72CI \
  --source rosca-admin \
  --network mainnet \
  -- upgrade \
  --new_wasm_hash <NEW_HASH>
```

#### Admin Transfer Procedure (two-step)

```bash
# Step 1: Current admin proposes new admin
stellar contract invoke \
  --id CCE7TPDTAZOFGU6ZCKDEBKBWWXKO27D4I5AHKYRXC2FUNATCPRFY72CI \
  --source rosca-admin \
  --network mainnet \
  -- transfer_admin \
  --new_admin <NEW_ADMIN_ADDRESS>

# Step 2: New admin accepts
stellar contract invoke \
  --id CCE7TPDTAZOFGU6ZCKDEBKBWWXKO27D4I5AHKYRXC2FUNATCPRFY72CI \
  --source <NEW_ADMIN_KEY> \
  --network mainnet \
  -- accept_admin
```

#### Testnet

```bash
# Deploy
stellar contract deploy \
  --wasm target/wasm32v1-none/release/rosca_v2.optimized.wasm \
  --source alice \
  --network testnet

# Initialize
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source alice \
  --network testnet \
  -- initialize \
  --admin <ADMIN_ADDRESS> \
  --config '{...}'
```

For the full configuration schema and detailed design rationale, see [`ROSCA_V2_DESIGN.md`](./ROSCA_V2_DESIGN.md).
