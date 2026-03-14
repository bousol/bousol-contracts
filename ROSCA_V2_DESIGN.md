# ROSCA V2 Contract Design Document

## 1. Project Overview

### 1.1 What Is ROSCA?

ROSCA (Rotating Savings and Credit Association) is one of the world's oldest community savings mechanisms. Known as **Sol** (Haiti), **Tontine** (West Africa), **Chit Fund** (India), **Tanda** (Latin America), **标会** (China/Taiwan).

**How it works:** A group of people contribute a fixed amount each round, and one member receives the entire pot. By the end of the cycle, everyone has contributed and received the same total amount — but at different times.

**The trust problem:** Traditionally, once a member receives the pot early, they may default on subsequent contributions. There is no enforcement mechanism other than social pressure.

**Bousol's solution:** Replace trust with a Stellar smart contract (Soroban). The contract holds all funds, enforces contribution rules, automatically penalizes violations, and distributes payouts — no single person controls the money.

**Simple example** — 3 friends, $100/week:

| Week | Everyone Pays | Beneficiary | Pot Size |
|------|--------------|-------------|----------|
| 1    | $100         | Alice       | $300     |
| 2    | $100         | Bob         | $300     |
| 3    | $100         | Charlie     | $300     |

Each person pays $300 total, receives $300 total — but at different times.

### 1.2 ROSCA V2 Core Features

- **Deposit System**: Members must pay a deposit before joining; used for default penalties and risk coverage
- **Dual Tracking System**:
  - Debt system (amount dimension): tracks contributed and received amounts
  - Points system (count dimension): determines payout priority and eligibility
- **Dynamic Membership**: Supports joining and leaving at any time (subject to conditions)
- **Default Protection**: Three-layer guarantee mechanism (defaulter's deposit → insurance pool → beneficiary bears loss)
- **Smart Priority**: Weighted-random payout selection based on contribution count and violation history
- **Observation Period**: New members must complete an observation period before receiving payouts
- **Progressive Penalties**: Escalating deposit deductions, point penalties, and lockout periods for violations
- **Late Fee System**: Progressive late fees with on-time streak discounts
- **Insurance Pool**: Percentage-based insurance fund for default coverage
- **Governance Proposals**: Decentralized voting for config updates, emergency payouts, and dissolution
- **Sponsor System**: Optional requirement for existing members to vouch for new joiners
- **Two-Step Exit**: Members request exit → processed at round settlement (no mid-round disruption)
- **Deposit Top-Up**: Members can replenish deposits after violation deductions

### 1.3 Design Principles

1. **Simplicity First**: Avoid complex cross-member cost-sharing; use the beneficiary-bears-loss model
2. **Fair and Transparent**: Each person's contributions and receipts are balanced; debt must be cleared before exiting
3. **Controlled Risk**: Multi-layer guarantee mechanism to reduce the impact of defaults
4. **Flexible Participation**: Supports dynamic joining/exiting with no fixed end time
5. **Decentralized Governance**: No single admin controls the contract; configuration changes require member voting

---

## 2. Core Mechanisms

### 2.1 Debt System (Amount Dimension)

Each member maintains a debt balance:

```
net_balance = total_contributed - total_received
```

- `total_contributed`: cumulative amount contributed
- `total_received`: cumulative amount received (payouts + emergency payouts)

**Key Rules:**
- `net_balance >= 0`: member may request exit
- `net_balance < 0`: member must continue contributing to repay the debt
- A member who received early has negative net_balance and is economically incentivized to keep contributing

**Purpose:**
- Ensures each person's contributions and receipts are balanced
- Used as the criterion for determining exit eligibility
- Complements the points system as a fairness guarantee at the monetary level

### 2.2 Points System (Count Dimension)

#### 2.2.1 Points Calculation Formula

```
priority_score = contribution_count
               - (receive_count × members_count)
               - cumulative_violation_penalty
```

Where `cumulative_violation_penalty` is calculated from the progressive `violation_penalties` configuration:

```rust
fn priority_score(&self, members_count: u32, config: &RoscaConfig) -> i32 {
    let mut violation_penalty = 0i32;
    for i in 0..self.violation_count {
        if let Some(penalty) = config.violation_penalties.get(i) {
            violation_penalty += penalty.points_deduction as i32;
        } else {
            // Beyond configured range: use last penalty
            if let Some(last) = config.violation_penalties.last() {
                violation_penalty += last.points_deduction as i32;
            }
        }
    }

    self.contribution_count as i32
        - (self.receive_count as i32 * members_count as i32)
        - violation_penalty
}
```

**Parameter Description:**
- `contribution_count`: number of contributions (+1 point each)
- `receive_count`: number of payouts received (-N points each, where N = active member count at time of receipt)
- `violation_count`: number of violations (progressive penalty from config)

**Core Principle: 1 point = 1 contribution**

#### 2.2.2 Points Rules

```
Contribute once (on time or within grace period): +1 point
Receive once (10-member pool): -10 points
Violate once (1st time, default config): -3 points
Violate twice (2nd time, default config): -5 points additionally
```

**Example (10-member ROSCA with default penalties):**
```
Alice:
  - Contributed 12 times: +12
  - Received 1 time: -10
  - Violations 0: 0
  Score = 12 - 10 - 0 = 2 ✅ Eligible to receive

Bob:
  - Contributed 11 times: +11
  - Received 1 time: -10
  - Violations 1: -3 (1st penalty)
  Score = 11 - 10 - 3 = -2 ❌ Not eligible (needs to contribute 3 more times)
```

#### 2.2.3 Role of Points

1. **Payout eligibility check**: `priority_score > 0` is required to be eligible for payout
2. **Weighted random selection**: Higher score = higher weight = higher probability of selection
3. **Violation penalty reflection**: Violations significantly reduce points
4. **Incentive for compliance**: Timely contributions accumulate an advantage

#### 2.2.4 Handling Negative Scores

**Internal implementation:** Negative scores are allowed (e.g., -9, -12)
- Simplifies calculation logic
- Clearly expresses debt status

**External display:** Avoid showing negative scores directly
```
❌ Poor display: "Points: -9"
✅ Good display: "Need to contribute 9 more times to receive a payout"

Frontend display example:
━━━━━━━━━━━━━━━━━━━━━━━━━━
📊 Alice's Status

📈 Contribution Progress
▓▓▓▓░░░░░░░░░░ 4/10 times (40%)
Contribute 6 more times to become eligible

⚠️ Violation Record: 1 time (penalty: 3 contributions)
💰 Deposit Balance: 4,000 / 5,000

📌 Next eligible to receive: need to contribute 9 more times
━━━━━━━━━━━━━━━━━━━━━━━━━━
```

#### 2.2.5 Impact of Violations on Points (Default Config)

| Violations | Points Penalty | Deposit Deduction | Lockout Period | Status |
|-----------|---------------|-------------------|----------------|--------|
| 1 time    | -3 points     | -1,000            | 2 rounds       | Warning |
| 2 times   | -5 points     | -2,000            | 5 rounds       | Serious |
| 3 times   | Kicked out    | All remaining     | -              | Kicked Out ❌ |

**Combined Effect:**
```
After Bob's 1st violation:
- Points: -3 (needs 3 additional contributions to offset)
- Deposit: deducted 1,000
- Lockout: cannot receive for 2 rounds
- Record: violation_count = 1

If Bob violates 2 more times → kicked out
```

#### 2.2.6 Voting Weight

Voting weight for governance proposals is based on contribution count:

```rust
fn voting_weight(&self) -> u32 {
    self.contribution_count
}
```

Members who have contributed more have proportionally more voting power. This prevents new or inactive members from controlling governance decisions.

### 2.3 Deposit Mechanism

#### 2.3.1 Deposit Range

```rust
min_deposit = contribution_amount × 3     // 3,000
recommended_deposit = contribution_amount × 5  // 5,000
max_deposit = contribution_amount × 10    // 10,000 (optional)
```

- Members may freely choose a deposit amount within the range
- The deposit does not affect payout weight; it serves only as a security bond
- If the deposit falls below the minimum after violations, the member should top it up

#### 2.3.2 Use of Deposits

1. **Violation penalty**: Deposit is deducted when a member defaults
2. **Compensation fund**: Used to compensate affected recipients
3. **Exit refund**: Fully refunded upon normal exit (along with positive net_balance)

#### 2.3.3 Deposit Top-Up

After violations deduct a member's deposit, they can replenish it via `top_up_deposit()`:

```rust
pub fn top_up_deposit(env, member: Address, amount: i128) -> Result<(), Error>
```

**Rules:**
- `amount > 0`
- `member.deposit + amount <= max_deposit` (if max_deposit is configured)
- Transfers tokens from member to contract
- Updates `member.deposit`

### 2.4 Insurance Pool Mechanism

#### 2.4.1 Insurance Pool Sources

```
Each contribution of 1,000:
- 980 → current round contribution pool (98%)
- 20 → insurance pool (2%)
```

Other sources for the insurance pool:
- Forfeited deposits (from violators, routed through compensation)
- Late fee income

#### 2.4.2 Insurance Pool Uses

- Compensate recipients (when a defaulter's deposit is insufficient)
- Buffer default risk and reduce beneficiary losses
- Ensure system stability

#### 2.4.3 Insurance Pool Cap

```rust
max_insurance_pool = contribution_amount × 20  // 20,000
max_insurance_coverage: i128,  // Maximum insurance compensation per round
```

**Insurance Pool Management:**
- Pool is capped at `max_insurance_pool` — enforced in both `contribute()` and `contribute_late()` (excess is silently capped)
- `max_insurance_coverage` caps the amount the pool can compensate in a single round
- When the contract dissolves: remaining insurance pool is distributed to all Active members equally

**Actual user cost:**
```
Ideal case (no defaults):
- Contributed 10 times = 10,000
- Insurance fees = 200 (2%)
- If contract dissolves: pool distributed proportionally
- Actual cost ≤ 2%

With defaults:
- Insurance fees used to compensate affected parties
- Actual cost ≤ 2%
```

### 2.5 Default Handling Mechanism

#### 2.5.1 Three-Layer Guarantee

When a member defaults, the current round's recipient is compensated in the following order:

```
1. Defaulter's deposit (deducted per violation_penalties config)
   ↓ insufficient
2. Insurance pool compensation (capped at max_insurance_coverage)
   ↓ still insufficient
3. Beneficiary bears the final loss (capped at max_beneficiary_loss_rate)
```

#### 2.5.2 Violation Penalties (Progressive)

**Progressive penalty mechanism:** The more violations, the heavier the penalty

```rust
1st violation (default config):
- Deposit deducted: 1,000
- Points deducted: -3
- Lockout period: 2 rounds
- Status: Warning ⚠️

2nd violation:
- Deposit deducted: 2,000
- Points deducted: -5 (cumulative -8)
- Lockout period: 5 rounds
- Status: Serious Warning 🔴

3rd violation:
- Status: Kicked Out ❌ (violation_count >= max_violations)
- Cannot participate again
```

**Penalty configuration (fully customizable per contract):**
```rust
struct ViolationPenalty {
    deposit_deduction: i128,    // deposit deduction amount
    points_deduction: u32,      // points deduction
    lockout_rounds: u64,        // number of lockout rounds
}
```

#### 2.5.3 Violation Cost Analysis

```
Bob's violation cost (10-member pool, already received 10,000):

1st violation:
- Deposit loss: 1,000
- Points penalty: -3 (needs 3 extra contributions = 3,000)
- Lockout period: 2 rounds (missed payout opportunities)
- Total cost: 4,000 + opportunity cost

2nd violation (cumulative):
- Deposit loss: 3,000 (1,000 + 2,000)
- Points penalty: -8 (3 + 5)
- Extra contributions needed: 8 times = 8,000
- Total cost: 11,000 (exceeds received amount) ❌

3rd violation:
- Kicked out, all deposit lost
- If net_balance < 0, cannot exit
- All invested funds lost ❌
```

#### 2.5.4 Grace Period and Late Fees

**Grace period settings:**
```
Contribution deadline: round_end_time
Grace period: violation_grace_period seconds (e.g., 24 hours)
Actual violation determination: round_end_time + violation_grace_period

Contributing within grace period:
- Does not count as a violation ✅
- Must pay a late fee (progressive)
- contribution_count increases normally by +1
- on_time_streak resets to 0
```

**Progressive late fees:**
```rust
late_fee_rates: Vec<u32>,  // e.g., [5, 10, 20] for 5%, 10%, 20%
max_late_count: u32,       // e.g., 3 — beyond this, treated as violation

fn calculate_late_fee(late_count: u32, contribution_amount: i128) -> i128 {
    let rate = late_fee_rates.get(late_count).unwrap_or(20);
    contribution_amount * rate / 100
}
```

**Late fee examples:**
```
Alice's late contribution record:

1st late:
- Late fee: 1,000 × 5% = 50
- Total paid: 1,050
- late_count: 1

2nd late:
- Late fee: 1,000 × 10% = 100
- Total paid: 1,100
- late_count: 2

3rd late:
- Late fee: 1,000 × 20% = 200
- Total paid: 1,200
- late_count: 3 ⚠️ Limit reached

4th late:
- max_late_count exceeded
- Cannot use contribute_late() ❌
- Treated as violation
```

**Where late fees go:**
```
All goes into the insurance pool (strengthens insurance capacity)
```

#### 2.5.5 On-Time Streak Discount

Members who consistently contribute on time accumulate an `on_time_streak` counter. If they eventually incur a late fee, the streak provides a discount:

```rust
fn calculate_late_fee(&self, base_fee: i128) -> i128 {
    if self.on_time_streak >= 20 {
        base_fee * 20 / 100   // 80% discount — pay only 20%
    } else if self.on_time_streak >= 10 {
        base_fee * 50 / 100   // 50% discount — pay only 50%
    } else {
        base_fee              // No discount
    }
}
```

**Rules:**
- On-time contribution: `on_time_streak += 1`
- Late contribution: `on_time_streak = 0` (reset)
- Violation (non-contribution): `on_time_streak = 0` (reset)

### 2.6 Cooldown Period Mechanism

After receiving a payout, a member enters a cooldown period during which they cannot receive again. Three types are supported:

```rust
enum CooldownType {
    FixedRounds(u64),     // Fixed number of rounds (e.g., 10 rounds)
    DynamicMembers,       // Dynamic = active member count at time of receipt
    TimeBased(u64),       // Time-based (seconds), converted to rounds
}
```

**Cooldown calculation:**
```rust
match config.cooldown_type {
    FixedRounds(rounds) => current_round + rounds,
    DynamicMembers => current_round + active_members_count,
    TimeBased(secs) => current_round + max(secs / contribution_period, 1),
}
```

**Example (DynamicMembers with 10 active members):**
```
Alice receives in Round 5:
- cooldown_until_round = 5 + 10 = 15
- Alice cannot receive again until Round 15

In Round 12, 2 new members join (12 total):
- Alice's cooldown unchanged (still 15) — locked at receipt time

In Round 15, Alice receives again:
- cooldown_until_round = 15 + 12 = 27 (now 12 members)
```

**Example (TimeBased with 90 days, weekly period):**
```
secs = 90 * 86400 = 7,776,000
contribution_period = 604,800 (7 days)
cooldown_rounds = 7,776,000 / 604,800 = 12 (rounds)

Alice receives in Round 5:
- cooldown_until_round = 5 + 12 = 17
```

### 2.7 Observation Period

New members may be required to complete an observation period before becoming eligible for payouts:

```rust
observation_contributions: u32,   // e.g., 3 contributions required
all_members_observation: bool,    // if true, even founding members start as Observing
```

**Flow:**
```
1. Member joins → status = Observing (if all_members_observation == true)
                → status = Active    (if all_members_observation == false)

2. Each time an Observing member contributes:
   - observation_count += 1
   - If observation_count >= observation_contributions:
     - status → Active

3. While Observing:
   - Must contribute each round (violation penalties apply)
   - Cannot receive payouts (can_receive() requires status == Active)
   - Observation contributions count toward contribution_count and points
```

**Purpose:**
- Prevents the creator from immediately receiving and disappearing
- Ensures every member demonstrates commitment before receiving
- Fair to all members when `all_members_observation = true`

### 2.8 Sponsor System

Optional mechanism requiring new members to be vouched for by an existing active member:

```rust
require_sponsor: bool,  // in RoscaConfig
```

**Flow:**
```
1. Config has require_sponsor = true

2. Active member calls sponsor(sponsor_addr, candidate_addr)
   - sponsor must be Active member
   - candidate must not already be a member
   - Stores: DataKey::Sponsor(candidate) → sponsor address (temporary)

3. Candidate calls join(candidate_addr, deposit_amount)
   - If require_sponsor: reads DataKey::Sponsor(candidate) or Error::SponsorRequired
   - Stores sponsor address in Member.sponsored_by (permanent audit trail)
   - Deletes temporary DataKey::Sponsor(candidate) record (storage cleanup)
   - Even without require_sponsor, if a voluntary sponsor record exists, it is stored and cleaned up
```

**Purpose:**
- Adds a social accountability layer
- Existing members stake their reputation on new joiners
- Prevents anonymous/unknown parties from joining
- Audit trail preserved in `Member.sponsored_by` after temporary storage is cleaned up

### 2.9 System Accounts

Members can be flagged as `is_system_account = true`:

```rust
pub is_system_account: bool,  // Backend service account
```

**System accounts:**
- Contribute normally each round
- Are excluded from payout selection (`can_receive()` returns false)
- Used for backend service accounts that maintain the pool but don't receive
- Do not affect member count for cooldown calculations (they are Active members)

---

## 3. Governance System

### 3.1 Overview

ROSCA V2 uses a decentralized governance model. There is **no admin** with unilateral control. All significant changes require member voting through proposals.

### 3.2 Proposal Types

| Type | Approval Threshold | Voting Period | Cooldown | Use Case |
|------|-------------------|---------------|----------|----------|
| `EmergencyPayout(details)` | >66% | 48 hours | None | Member needs urgent funds |
| `UpdateConfig(new_config)` | >50% | 7 days | 7 days | Change contribution amount, fees, etc. |
| `Dissolution(Emergency)` | >75% | 24 hours | None | Critical emergency dissolution |
| `Dissolution(Normal)` | >90% | 14 days | None | Orderly wind-down |

### 3.3 Proposal Lifecycle

```
1. Active member calls propose(proposer, proposal_type)
   → Creates Proposal with voting_ends_at and optional cooldown_ends_at
   → Returns proposal_id

2. Active members call vote(voter, proposal_id, choice)
   → choice: For or Against
   → Each voter's weight = their contribution_count (voting_weight)
   → Cannot vote twice on the same proposal

3. After voting period ends, any active member calls execute_proposal(executor, proposal_id)
   → Checks: voting ended, cooldown ended (if applicable), threshold met
   → Executes the proposal action
   → Marks proposal as executed
```

### 3.4 Proposal Details

#### 3.4.1 Emergency Payout

A member in financial distress can request an emergency payout from their own net_balance:

```rust
ProposalType::EmergencyPayout(EmergencyPayoutDetails {
    requester: Address,  // Who receives the emergency payout
    amount: i128,        // How much (must be <= requester.net_balance)
})
```

**On proposal creation:**
- Validates `amount > 0` (rejects zero or negative amounts)

**On execution:**
- Verifies `requester.net_balance() >= amount`
- Verifies contract token balance >= `amount` (prevents panic if funds are insufficient)
- Transfers `amount` from contract to requester
- Updates requester: `total_received += amount`, `receive_count += 1`
- Sets cooldown period (same as normal payout)

#### 3.4.2 Update Config

Members can propose changing the ROSCA configuration:

```rust
ProposalType::UpdateConfig(new_config: RoscaConfig)
```

**On execution:**
- Validates new config (`validate_config`)
- **Ensures `token_address` is not changed** (immutable — prevents fund theft)
- Replaces stored config with new config
- Has a 7-day cooldown after voting ends before execution (safety buffer)

**What can be changed via UpdateConfig:**
- `contribution_amount`, `contribution_period`
- `min_deposit`, `recommended_deposit`, `max_deposit`
- `insurance_rate`, `max_insurance_pool`, `max_insurance_coverage`
- `observation_contributions`, `all_members_observation`
- `cooldown_type`
- `violation_grace_period`, `violation_penalties`, `max_violations`
- `late_fee_rates`, `max_late_count`
- `max_beneficiary_loss_rate`
- `allow_join`, `require_sponsor`

**What CANNOT be changed:**
- `token_address` (immutable — hardcoded validation)
- `status` (preserved from current config — status changes must use Dissolution proposal to prevent bypassing governance thresholds)

#### 3.4.3 Dissolution

Two modes for winding down the ROSCA:

**Emergency Dissolution** (>75%, 24h):
- For critical situations (e.g., security vulnerability, token delist)
- Fast track with high threshold

**Normal Dissolution** (>90%, 14d):
- Orderly wind-down with near-unanimous consent
- Longer deliberation period

**On execution (both modes):**
1. Distribute insurance pool equally among Active members (`share = pool / active_count`)
2. Calculate remaining contract balance after insurance distribution
3. Calculate each member's claim: `deposit + max(0, net_balance)`
4. Refund members (pro-rata if total claims exceed remaining balance — prevents panic)
5. Set `config.status = Dissolved`

### 3.5 Voting Weight

```rust
fn voting_weight(&self) -> u32 {
    self.contribution_count
}
```

- Members who have contributed more have proportionally more say
- New members (few contributions) have less influence
- Prevents governance capture by inactive or new members

---

## 4. Data Structure Design

### 4.1 Contract Configuration

```rust
struct RoscaConfig {
    // Basic parameters
    contribution_amount: i128,        // contribution amount per round (e.g., 1,000)
    contribution_period: u64,         // contribution cycle (in seconds, e.g., 604,800 = 7 days)

    // Deposit configuration
    min_deposit: i128,                // minimum deposit (3,000)
    recommended_deposit: i128,        // recommended deposit (5,000)
    max_deposit: Option<i128>,        // maximum deposit (optional, 10,000)

    // Insurance mechanism
    insurance_rate: u32,              // insurance rate (2 = 2%)
    max_insurance_pool: i128,         // insurance pool cap (20,000)
    max_insurance_coverage: i128,     // maximum insurance compensation per round (2,000)

    // Observation period
    observation_contributions: u32,   // required contributions during observation period (3)
    all_members_observation: bool,    // whether all members require an observation period (true)

    // Cooldown period
    cooldown_type: CooldownType,      // FixedRounds(u64) | DynamicMembers | TimeBased(u64)

    // Violation configuration (progressive)
    violation_grace_period: u64,      // grace period (in seconds, e.g., 86,400 = 24 hours)
    violation_penalties: Vec<ViolationPenalty>,  // progressive penalty configuration
    max_violations: u32,              // maximum violations allowed (3 = kicked on 3rd)

    // Late fee configuration (progressive)
    late_fee_rates: Vec<u32>,         // late fee rates ([5, 10, 20] for %)
    max_late_count: u32,              // maximum late count (3)

    // Beneficiary protection
    max_beneficiary_loss_rate: u32,   // maximum loss rate (10 = 10%)

    // Administration
    allow_join: bool,                 // whether new members may join
    require_sponsor: bool,            // whether new members need a sponsor

    // Status
    status: RoscaStatus,              // Active | Paused | Dissolved

    // Token
    token_address: Address,           // Token contract address (USDC SAC)
}

struct ViolationPenalty {
    deposit_deduction: i128,          // deposit deduction amount
    points_deduction: u32,            // points deduction
    lockout_rounds: u64,              // number of lockout rounds
}

enum CooldownType {
    FixedRounds(u64),      // fixed number of rounds (e.g., 10 rounds)
    DynamicMembers,        // dynamic = active member count at time of receipt
    TimeBased(u64),        // time-based in seconds, converted to rounds via (secs / contribution_period).max(1)
}

enum RoscaStatus {
    Active,     // Normal operation
    Paused,     // Paused (can be resumed via UpdateConfig proposal)
    Dissolved,  // Dissolved (terminal state)
}
```

### 4.2 Member Data

```rust
struct Member {
    // Basic information
    address: Address,
    joined_at: u64,                   // join timestamp
    status: MemberStatus,
    is_system_account: bool,          // backend service account (excluded from payouts)

    // === Debt system (amount dimension) ===
    deposit: i128,                    // current deposit balance
    total_contributed: i128,          // cumulative amount contributed
    total_received: i128,             // cumulative amount received

    // === Points system (count dimension) ===
    contribution_count: u32,          // number of contributions
    receive_count: u32,               // number of payouts received
    violation_count: u32,             // number of violations
    late_count: u32,                  // number of late contributions
    on_time_streak: u32,              // consecutive on-time contributions (for late fee discount)

    // === Statistics ===
    observation_count: u32,           // contributions during observation period
    last_contribution_round: u64,     // last round contributed (u64::MAX = never)
    last_received_round: u64,         // last round received (u64::MAX = never)

    // === State control ===
    cooldown_until_round: u64,        // round at which cooldown period ends
    violation_lockout_until: u64,     // round at which violation lockout ends

    // === Sponsorship ===
    sponsored_by: Option<Address>,    // who sponsored this member (audit trail, None if no sponsor)
}

enum MemberStatus {
    Observing,      // In observation period (contributes but cannot receive)
    Active,         // Full active member
    ExitPending,    // Exit requested (will be processed at next settle_round)
    Kicked,         // Kicked out due to max violations
}

// Computed properties
impl Member {
    // Debt balance (amount)
    fn net_balance(&self) -> i128 {
        self.total_contributed - self.total_received
    }

    // Priority score (count) — uses progressive violation penalties from config
    fn priority_score(&self, members_count: u32, config: &RoscaConfig) -> i32 { ... }

    // Payout eligibility (all conditions must be met)
    fn can_receive(&self, env, current_round: u64, members_count: u32, config: &RoscaConfig) -> bool {
        !self.is_system_account                              // Not a system account
        && self.status == MemberStatus::Active               // Must be Active
        && self.net_balance() >= 0                           // Debt cleared
        && self.priority_score(members_count, config) > 0   // Positive score
        && self.observation_count >= config.observation_contributions  // Observation complete
        && current_round >= self.cooldown_until_round        // Cooldown ended
        && current_round >= self.violation_lockout_until     // Violation lockout ended
    }

    // Exit condition
    fn can_exit(&self) -> bool {
        self.net_balance() >= 0
    }

    // Voting weight for governance
    fn voting_weight(&self) -> u32 {
        self.contribution_count
    }

    // Late fee with on-time streak discount
    fn calculate_late_fee(&self, base_fee: i128) -> i128 {
        if self.on_time_streak >= 20 { base_fee * 20 / 100 }   // 80% off
        else if self.on_time_streak >= 10 { base_fee * 50 / 100 } // 50% off
        else { base_fee }
    }
}
```

### 4.3 Round Data

```rust
struct Round {
    round_id: u64,
    start_time: u64,
    end_time: u64,

    // Participants
    expected_contributors: Vec<Address>,  // Active + Observing members expected to contribute
    actual_contributors: Vec<Address>,    // Members who actually contributed
    violators: Vec<Address>,              // Members who violated

    // Funds
    total_collected: i128,                // actual amount collected
    insurance_collected: i128,            // insurance fees collected
    recipient: Option<Address>,           // payout recipient (None if no eligible candidate)
    payout_amount: i128,                  // actual payout amount

    // Compensation breakdown
    violations_loss: i128,                // loss from violations
    deposit_compensation: i128,           // compensation from violators' deposits
    insurance_compensation: i128,         // compensation from insurance pool
    beneficiary_loss: i128,               // remaining loss borne by the beneficiary
}
```

### 4.4 Governance Data

```rust
struct Proposal {
    id: u64,
    proposer: Address,
    proposal_type: ProposalType,
    created_at: u64,
    voting_ends_at: u64,
    votes_for_weight: u32,          // total weight of "For" votes
    votes_against_weight: u32,      // total weight of "Against" votes
    executed: bool,
    cooldown_ends_at: Option<u64>,  // for UpdateConfig (7d cooldown after voting)
}

enum ProposalType {
    EmergencyPayout(EmergencyPayoutDetails),
    UpdateConfig(RoscaConfig),
    Dissolution(DissolutionMode),
}

struct EmergencyPayoutDetails {
    requester: Address,
    amount: i128,
}

enum DissolutionMode {
    Emergency,  // >75%, 24h
    Normal,     // >90%, 14d
}

enum VoteChoice {
    For,
    Against,
}

struct Vote {
    voter: Address,
    proposal_id: u64,
    choice: VoteChoice,
    weight: u32,       // voting weight at time of vote
    voted_at: u64,
}
```

### 4.5 Statistics

```rust
struct Statistics {
    total_rounds: u64,
    total_members: u32,
    active_members: u32,
    total_contributed: i128,
    total_paid_out: i128,
    insurance_pool: i128,
    total_violations: u32,
}
```

### 4.6 Storage Keys

```rust
enum DataKey {
    // Configuration
    Config,                    // RoscaConfig

    // State
    CurrentRound,              // u64
    StartTime,                 // u64
    InsurancePool,             // i128

    // Members
    Member(Address),           // Member
    MembersList,               // Vec<Address>

    // Rounds
    Round(u64),                // Round

    // Statistics
    Statistics,                // Statistics

    // Sponsorship
    Sponsor(Address),          // Address (sponsor address for a candidate)

    // Governance
    ProposalCounter,           // u64 (auto-increment)
    Proposal(u64),             // Proposal by ID
    Vote(u64, Address),        // Vote by (proposal_id, voter_address)
}
```

---

## 5. Core Flows

### 5.1 Initialization Flow

```
1. Deploy contract
2. Call initialize(config: RoscaConfig)
   ├─ Validate: not already initialized
   ├─ Validate config:
   │   ├─ contribution_amount > 0
   │   ├─ contribution_period > 0
   │   ├─ min_deposit > 0 && min_deposit <= recommended_deposit
   │   ├─ insurance_rate <= 100
   │   ├─ max_beneficiary_loss_rate <= 100
   │   ├─ max_deposit >= min_deposit (if max_deposit is set)
   │   └─ violation_penalties must not be empty
   ├─ Store config
   ├─ Set CurrentRound = 0, StartTime = now, InsurancePool = 0
   ├─ Initialize empty MembersList
   └─ Initialize Statistics (all zeros)
```

### 5.2 Join Flow

```
1. (Optional) If require_sponsor:
   ├─ Existing active member calls sponsor(sponsor_addr, candidate_addr)
   └─ Stores DataKey::Sponsor(candidate) → sponsor address

2. User calls join(member, deposit_amount)
   ├─ Validate: status == Active
   ├─ Validate: allow_join == true
   ├─ Validate: not already a member
   ├─ Read sponsor: if require_sponsor → read Sponsor(member) or Error
   │   (also reads voluntary sponsor if present)
   ├─ Validate: deposit_amount >= min_deposit
   ├─ Validate: deposit_amount <= max_deposit (if set)
   ├─ Transfer deposit from member to contract
   ├─ Create Member:
   │   ├─ status = Observing (if all_members_observation) or Active
   │   ├─ deposit = deposit_amount
   │   ├─ All counters = 0
   │   ├─ last_contribution_round = u64::MAX
   │   ├─ last_received_round = u64::MAX
   │   └─ sponsored_by = sponsor address (or None)
   ├─ Clean up temporary Sponsor(member) storage key
   ├─ Add to MembersList
   └─ Update Statistics (total_members++, active_members++ only if not all_members_observation)

3. Observation period (automatic)
   ├─ Member contributes normally N times
   ├─ observation_count++ each time
   └─ Once observation_count >= observation_contributions: status → Active
```

### 5.3 Contribution Flow

```
At the start of each round:
1. Calculate round timing:
   - round_start = start_time + (current_round × contribution_period)
   - round_end = round_start + contribution_period

2. Members contribute within the period:

Normal contribution (within round_start to round_end):
   Call contribute(member)
   ├─ Validate: status == Active or Observing (ExitPending excluded)
   ├─ Validate: not already contributed this round
   ├─ Validate: current_time within [round_start, round_end]
   ├─ Calculate: insurance_amount = contribution_amount × insurance_rate / 100
   ├─ Transfer contribution_amount from member to contract
   ├─ Update member:
   │   ├─ total_contributed += contribution_amount
   │   ├─ contribution_count++
   │   ├─ last_contribution_round = current_round
   │   ├─ on_time_streak++ ✅
   │   └─ If Observing: observation_count++ (→ Active if threshold met)
   ├─ Update InsurancePool += insurance_amount
   └─ Update Statistics

Late contribution (within round_end to round_end + grace_period):
   Call contribute_late(member)
   ├─ Validate: late_count < max_late_count
   ├─ Validate: current_time within (round_end, round_end + violation_grace_period]
   ├─ Calculate late fee: base_rate from late_fee_rates[late_count]
   ├─ Apply on_time_streak discount
   ├─ Transfer contribution_amount + late_fee from member
   ├─ Update member:
   │   ├─ total_contributed += contribution_amount (fee not counted)
   │   ├─ contribution_count++
   │   ├─ late_count++
   │   ├─ on_time_streak = 0 ❌ (reset)
   │   └─ If Observing: observation_count++
   ├─ Update InsurancePool += insurance_amount + late_fee
   └─ Update Statistics
```

### 5.4 Settlement Flow (settle_round)

```
Anyone can call settle_round(random_seed) after round_end + grace_period:
(settle_round waits for the full grace period so members can use contribute_late)

1. Identify expected contributors (Active + Observing members)
   - ExitPending members are NOT expected to contribute (no violation for them)

2. Determine contributors vs. violators:
   - Contributed: last_contribution_round == current_round → actual contributor
   - Did not contribute: → violator

3. Process violations:
   For each violator:
   ├─ violation_count++
   ├─ on_time_streak = 0
   ├─ If violation_count >= max_violations:
   │   ├─ Confiscate ALL remaining deposit (deposit → 0, added to compensation pool)
   │   ├─ status = Kicked
   │   └─ If member was Active: counted in kicked_count for stats decrement
   ├─ Else apply progressive penalty:
   │   ├─ Deduct deposit (capped at available deposit)
   │   ├─ Record deposit compensation amount
   │   └─ Set violation_lockout_until = current_round + lockout_rounds
   └─ Save updated member

4. Select recipient via weighted random:
   ├─ Filter: all members where can_receive() == true
   ├─ Weight = max(priority_score, 1)
   ├─ Total weight = sum of all candidates' weights
   ├─ random_value = random_seed % total_weight
   ├─ Walk through candidates accumulating weight
   └─ First candidate where accumulated >= random_value wins

5. Calculate payout (if recipient found):
   ├─ ideal_payout = total_collected - insurance_collected
   ├─ violations_loss = violator_count × contribution_amount
   ├─ actual_available = ideal_payout - violations_loss
   ├─ compensation_needed = ideal_payout - actual_available
   │
   ├─ Step 1: Deposit compensation
   │   └─ deposit_comp = min(compensation_needed, total_deposit_deductions)
   ├─ Step 2: Insurance compensation
   │   └─ insurance_comp = min(remaining, insurance_pool, max_insurance_coverage)
   ├─ Step 3: Beneficiary loss
   │   └─ beneficiary_loss = min(remaining, ideal_payout × max_beneficiary_loss_rate / 100)
   │
   ├─ final_payout = max(0, actual_available + deposit_comp + insurance_comp)
   │   (clamped to 0 to prevent negative payout in extreme scenarios)
   ├─ Transfer final_payout to recipient (if > 0)
   └─ Update recipient:
       ├─ total_received += final_payout
       ├─ receive_count++
       ├─ last_received_round = current_round
       └─ cooldown_until_round = current_round + cooldown

6. If NO recipient found:
   ├─ Insurance fee still collected
   ├─ Refund (contribution - insurance) to each contributor
   └─ No one receives the pot this round

7. Create Round record with full breakdown

8. Update Statistics (total_rounds++, total_violations, total_paid_out)

9. Advance round: CurrentRound += 1

10. Process ExitPending members (pro-rata safe):
    ├─ Calculate total exit claims across all ExitPending members
    ├─ Check contract token balance
    ├─ For each member with status == ExitPending:
    │   ├─ claim = deposit + max(0, net_balance)
    │   ├─ refund = claim (or pro-rata scaled if total claims > contract balance)
    │   ├─ Transfer refund to member
    │   ├─ Remove member from MembersList
    │   └─ Delete Member data
    └─ Note: active_members was already decremented in request_exit()
```

### 5.5 Exit Flow (Two-Step)

```
Step 1: Member calls request_exit(member)
   ├─ Validate: status == Active or Observing
   ├─ Validate: net_balance() >= 0 (debt cleared)
   ├─ Set status = ExitPending
   └─ If member was Active: active_members-- immediately
     (Observing members were never counted in active_members)

Step 2: At next settle_round (automatic)
   ├─ ExitPending members are skipped in contribution expectations
   ├─ ExitPending members cannot receive payouts (can_receive requires Active)
   ├─ After round advances (pro-rata safe refund):
   │   ├─ Calculate each exit claim: deposit + max(0, net_balance)
   │   ├─ Check contract balance; if total claims exceed balance → pro-rata scale
   │   ├─ Transfer refund to member
   │   ├─ Remove from MembersList
   │   └─ Delete member data
   └─ Note: active_members already decremented at request_exit() time

Benefits of two-step:
- No mid-round disruption to contribution counts
- ExitPending member is excluded from that round's expected contributors
- Clean separation: request intent → execute at boundary
```

### 5.6 Governance Flow

```
1. Propose:
   Active member calls propose(proposer, proposal_type)
   ├─ Creates Proposal with voting_ends_at
   ├─ For UpdateConfig: cooldown_ends_at = voting_ends + 7 days
   └─ Returns proposal_id

2. Vote:
   Active members call vote(voter, proposal_id, choice)
   ├─ Validates: voting period not ended, not already voted
   ├─ Weight = voter.contribution_count
   └─ Updates proposal.votes_for_weight or votes_against_weight

3. Execute:
   Any active member calls execute_proposal(executor, proposal_id)
   ├─ Validates: voting ended, cooldown ended (if applicable)
   ├─ Calculates total_voting_weight across all active members
   ├─ Checks approval threshold (all cast to u64 to prevent u32 overflow):
   │   ├─ EmergencyPayout: votes_for_weight as u64 × 100 > total as u64 × 66
   │   ├─ UpdateConfig: votes_for_weight as u64 × 100 > total as u64 × 50
   │   ├─ Dissolution(Emergency): votes_for_weight as u64 × 100 > total as u64 × 75
   │   └─ Dissolution(Normal): votes_for_weight as u64 × 100 > total as u64 × 90
   ├─ Executes action
   └─ Marks proposal.executed = true
```

### 5.7 Dissolution Flow

```
Via execute_proposal after successful Dissolution vote:

1. Distribute insurance pool to Active members:
   ├─ share = insurance_pool / active_member_count
   └─ Transfer share to each Active member

2. Calculate remaining contract balance (after insurance distribution)

3. Refund all members (pro-rata safe):
   For each member:
   ├─ claim = deposit + max(0, net_balance)
   ├─ If total_claims > remaining_balance:
   │   └─ refund = claim × remaining_balance / total_claims (pro-rata)
   ├─ Else: refund = claim
   └─ Transfer refund to member

4. Set config.status = Dissolved

Post-dissolution:
- No operations allowed (all public functions check status)
- Contract is effectively frozen
```

---

## 6. Contract Interface Design

### 6.1 Initialization

```rust
pub fn initialize(env: Env, config: RoscaConfig) -> Result<(), Error>
```

### 6.2 Query Functions (Read-Only)

```rust
pub fn get_config(env: Env) -> Result<RoscaConfig, Error>
pub fn get_current_round(env: Env) -> Result<u64, Error>
pub fn get_insurance_pool(env: Env) -> Result<i128, Error>
pub fn get_member(env: Env, address: Address) -> Result<Member, Error>
pub fn get_members(env: Env) -> Result<Vec<Address>, Error>
pub fn get_statistics(env: Env) -> Result<Statistics, Error>
pub fn get_round(env: Env, round_id: u64) -> Result<Round, Error>
pub fn get_proposal(env: Env, proposal_id: u64) -> Result<Proposal, Error>
pub fn get_vote(env: Env, proposal_id: u64, voter: Address) -> Result<Vote, Error>
pub fn calculate_recipient(env: Env) -> Result<Option<Address>, Error>  // deterministic highest-priority (for reference)
```

### 6.3 Member Management

```rust
// Sponsor a candidate (sponsor must be Active member)
pub fn sponsor(env: Env, sponsor: Address, candidate: Address) -> Result<(), Error>

// Join the ROSCA (deposit required)
pub fn join(env: Env, member: Address, deposit_amount: i128) -> Result<(), Error>

// Request exit (two-step: sets ExitPending, processed at next settle_round)
pub fn request_exit(env: Env, member: Address) -> Result<(), Error>

// Top up deposit (replenish after violations)
pub fn top_up_deposit(env: Env, member: Address, amount: i128) -> Result<(), Error>
```

### 6.4 Contributions

```rust
// Normal contribution (within round period)
pub fn contribute(env: Env, member: Address) -> Result<(), Error>

// Late contribution with late fee (within grace period)
pub fn contribute_late(env: Env, member: Address) -> Result<(), Error>
```

### 6.5 Round Settlement

```rust
// Settle current round (permissionless — anyone can call after round ends)
pub fn settle_round(env: Env, random_seed: u64) -> Result<(), Error>
```

### 6.6 Governance

```rust
// Create a proposal
pub fn propose(env: Env, proposer: Address, proposal_type: ProposalType) -> Result<u64, Error>

// Vote on a proposal
pub fn vote(env: Env, voter: Address, proposal_id: u64, choice: VoteChoice) -> Result<(), Error>

// Execute a passed proposal
pub fn execute_proposal(env: Env, executor: Address, proposal_id: u64) -> Result<(), Error>
```

---

## 7. Detailed Examples

### Example 1: Normal Flow — Alice's Complete Journey

**Initial state:**
- 10-member ROSCA
- Contribution amount: 1,000 per round
- Alice's deposit: 5,000

```
=== Round 1 ===
Alice joins:
  - deposit: 5,000
  - status: Observing
  - contribution_count: 0
  - receive_count: 0
  - priority_score: 0

Alice contributes:
  - Transfers in 1,000 → 980 contribution pool, 20 insurance pool
  - total_contributed: 1,000
  - contribution_count: 1
  - observation_count: 1
  - on_time_streak: 1
  - priority_score: 1 - 0 - 0 = 1
  - net_balance: 1,000

Bob receives:
  - All 10 members contributed; Bob receives 9,800 (10 × 980)

=== Rounds 2–3 ===
Alice continues contributing 2 more times:
  - contribution_count: 3
  - total_contributed: 3,000
  - observation_count: 3 ✅ Observation period passed
  - status: Active
  - on_time_streak: 3
  - priority_score: 3 - 0 - 0 = 3
  - net_balance: 3,000

=== Round 4 ===
Alice meets payout conditions (selected via weighted random):
  - priority_score: 3 > 0 ✅
  - net_balance: 3,000 >= 0 ✅
  - Observation period passed ✅
  - Selected as this round's recipient

Round ends:
  - Alice contributes: contribution_count = 4
  - All 10 members contributed; 9,800 collected
  - Alice receives 9,800
  - receive_count: 1
  - total_received: 9,800
  - priority_score: 4 - (1×10) - 0 = -6 ❌
  - net_balance: 4,000 - 9,800 = -5,800
  - cooldown_until_round: 14 (current 4 + 10 members)

=== Rounds 5–10 ===
Alice continues contributing 6 more times:
  - contribution_count: 10
  - total_contributed: 10,000
  - priority_score: 10 - 10 - 0 = 0 ❌ (must be > 0)
  - net_balance: 10,000 - 9,800 = 200 ✅

=== Round 11 ===
Alice contributes once more:
  - contribution_count: 11
  - priority_score: 11 - 10 - 0 = 1 ✅
  - net_balance: 1,200
  - Can receive again or exit

=== Round 12 ===
Alice requests exit via request_exit():
  - net_balance: 1,200 >= 0 ✅
  - status → ExitPending
  - Next settle_round processes exit:
    - Refund: deposit (5,000) + net_balance (1,200) = 6,200
    - Transfer 6,200 to Alice
    - Remove from members list

Final result:
  - Total in: 5,000 (deposit) + 11,000 (contributions) = 16,000
  - Received: 9,800
  - Refunded: 6,200
  - Net gain: 0 ✅
  - Actual cost: 11 × 20 = 220 (insurance fees)
```

---

### Example 2: Violation Scenario — Single Violation, Deposit Sufficient

**Background:**
- 10-member ROSCA
- Round 5, Carol selected as recipient
- Bob defaults (did not contribute)

```
=== Start of Round 5 ===
Expected:
  - 10 members should contribute
  - Carol awaiting payout

Actual contributions:
  - 9 members contribute: 9 × 980 = 8,820
  - Bob defaults: 0
  - Contribution pool: 8,820

=== Violation Processing ===

1. Calculate loss:
   violation_loss = 1 × contribution_amount = 1,000

2. Penalize Bob:
   - Bob.deposit: 5,000 → 4,000 (deducted 1,000)
   - Bob.violation_count: 0 → 1
   - Bob.on_time_streak: reset to 0
   - Bob.violation_lockout_until = 5 + 2 = 7

3. Compensate Carol:
   Step 1: From Bob's deposit deduction
     - Needed: shortfall
     - Available from deposit: 1,000 ✅

   Step 2: Insurance pool compensation
     - Not needed ✅

   Step 3: Carol bears loss
     - 0 ✅

4. Final result:
   - Carol receives full payout ✅
   - Bob.deposit: 4,000 (deducted 1,000)
   - Bob cannot receive until Round 7 (lockout)
```

---

### Example 3: Violation Scenario — Deposit Insufficient, Insurance Pool Used

**Background:**
- Bob has violated twice; only 600 deposit remaining
- Violates again in Round 8

```
=== Round 8 ===
Actual contributions:
  - 9 members contribute: 8,820
  - Bob defaults: 0

=== Violation Processing ===

1. Bob.violation_count: 2 → 3 (>= max_violations)
   - Status: Kicked ❌
   - Bob.deposit: 600 → 0 (all deducted)

2. Compensate recipient:
   Step 1: From Bob's deposit
     - Available: 600
     - Remaining shortfall: needs more

   Step 2: Insurance pool
     - Pool balance: 1,500
     - Compensation: covers shortfall ✅
     - Pool reduced

   Step 3: Recipient bears loss
     - 0 ✅

3. Final result:
   - Recipient receives full payout ✅
   - Bob kicked out permanently
```

---

### Example 4: Multiple Violations, Beneficiary Bears Partial Loss

**Background:**
- Round 10, 3 members violate simultaneously
- Insurance pool balance: 500 (low)
- All 3 violators have only 200 deposit remaining each

```
=== Round 10 ===
Actual contributions:
  - 7 members contribute: 7 × 980 = 6,860
  - 3 members default

=== Compensation Flow ===

1. Deduct deposits:
   - 3 × 200 = 600 total from deposits

2. Insurance pool:
   - Available: 500
   - Compensate: 500

3. Remaining shortfall:
   - Loss: violations_loss - deposit_comp - insurance_comp
   - beneficiary_loss = remaining (capped at max_beneficiary_loss_rate)

4. Final result:
   - Recipient receives reduced payout
   - All 3 violators kicked out
   - Loss recorded in Round data for transparency
```

---

### Example 5: Dynamic Membership Changes

**Background:**
- Initially 10 members
- Round 5: 2 members exit
- Round 8: 3 new members join

```
=== Rounds 1–4 ===
10 members running:
  - Collected per round: 10 × 980 = 9,800
  - 4 members have already received

=== Before Round 5 ===
Eve and Frank request exit (debt cleared):
  - status → ExitPending
  - At settle_round: refunded and removed
  - 8 members remaining

=== Round 5 ===
Grace receives:
  - 8 members contribute: 7,840
  - Grace receives: 7,840 ✅
  - Grace needs to repay: 7,840 (8 contributions)

=== Round 8 ===
3 new members join (Helen, Iris, Jack):
  - Status: Observing (if all_members_observation)
  - Total members: 11 (8 existing + 3 new)
  - New members must also contribute during observation

Round 9:
  - Collected per round: 11 × 980 = 10,780

Round 11:
  - New members pass observation period (3 contributions)
  - May be selected for payout

=== Results ===
Payout amounts differ across cohorts:
  - Rounds 1–4: 10 members → received ~9,800
  - Rounds 5–7: 8 members → received ~7,840
  - Round 8+: 11 members → received ~10,780

But everyone is treated fairly:
  - Receive as much as you repay (net_balance tracks this)
  - May exit when net_balance >= 0
```

---

### Example 6: Cooldown Period — All Three Types

**FixedRounds(10):**
```
Alice receives in Round 5:
  - cooldown_until_round = 5 + 10 = 15
  - Cannot receive again until Round 15
  - Regardless of member count changes
```

**DynamicMembers (10 active members):**
```
Alice receives in Round 5:
  - cooldown_until_round = 5 + 10 = 15

In Round 12, 2 new members join (12 total):
  - Alice's cooldown unchanged (still 15)

In Round 15, Alice receives again:
  - cooldown_until_round = 15 + 12 = 27 (12 members now)
```

**TimeBased(1,814,400 seconds = 3 weeks):**
```
contribution_period = 604,800 (1 week)
cooldown_rounds = 1,814,400 / 604,800 = 3

Alice receives in Round 5:
  - cooldown_until_round = 5 + 3 = 8
  - Cannot receive again until Round 8
```

---

### Example 7: Observation Period — All Members

**Background:**
- all_members_observation = true
- observation_contributions = 3
- 5 initial members

```
=== Round 0 (at creation) ===
5 members join:
  - Status: all Observing

=== Round 1 ===
All 5 contribute:
  - observation_count: 1
  - No one can receive (still Observing, and can_receive requires Active)

=== Rounds 2–3 ===
All 5 continue contributing:
  - observation_count: 3 → status: Active ✅

=== Round 4 ===
Now eligible for payout selection:
  - priority_score: 4 - 0 - 0 = 4 ✅
  - Weighted random selects one member

Significance:
  - Prevents hit-and-run (join → receive → disappear)
  - Everyone proves commitment equally
```

---

### Example 8: Grace Period and Late Fees with On-Time Streak

**Background:**
- Contribution deadline: round_end
- Grace period: 24 hours
- Late fee rate: 5% (1st late)
- Alice has on_time_streak = 25 (80% discount)

```
=== Round 10 ===

Alice misses the deadline but is within grace period:

  Alice calls contribute_late():
  - Base late fee: 1,000 × 5% = 50
  - On-time streak discount (25 streak): 80% off
  - Actual late fee: 50 × 20% = 10
  - Total paid: 1,000 + 10 = 1,010
  - on_time_streak: 25 → 0 ❌ (reset)
  - late_count: 0 → 1
  - ✅ Does NOT count as a violation
  - contribution_count still increases by 1

If Alice misses the grace period entirely:
  - ❌ Treated as violation
  - Deposit deducted, points penalized, lockout applied
```

---

### Example 9: Points Priority Ranking and Weighted Random Selection

**Background:**
- 10-member ROSCA, Round 12
- settle_round called with random_seed

```
=== Eligible Candidates ===

Alice (model member):
  - contribution_count: 12, receive_count: 1, violation_count: 0
  - priority_score: 12 - 10 - 0 = 2
  - weight: 2

Bob (occasional violations):
  - contribution_count: 14, receive_count: 1, violation_count: 1
  - priority_score: 14 - 10 - 3 = 1
  - weight: 1

Carol (frequent contributor):
  - contribution_count: 13, receive_count: 1, violation_count: 0
  - priority_score: 13 - 10 - 0 = 3
  - weight: 3

Dave (multiple violations):
  - priority_score: -1 ❌ Not eligible

Eve (observation period):
  - observation_count: 2 < 3 ❌ Not eligible

=== Weighted Random Selection ===

Candidates: Alice(2), Bob(1), Carol(3)
Total weight: 6

random_value = random_seed % 6

  0-2 → Carol  (weight 3, probability 50%)
  3-4 → Alice  (weight 2, probability 33%)
  5   → Bob    (weight 1, probability 17%)

Higher priority = higher probability, but not deterministic ✅
```

---

### Example 10: Sponsor Flow

```
=== Setup ===
ROSCA with require_sponsor = false initially
Member1 joins normally

=== Enable Sponsor Requirement ===
Members propose UpdateConfig with require_sponsor = true
Vote passes → config updated

=== New Candidate Wants to Join ===

1. Candidate asks Member1 to sponsor them
2. Member1 calls sponsor(member1, candidate)
   - Validates Member1 is Active ✅
   - Stores Sponsor(candidate) → member1 (temporary)

3. Candidate calls join(candidate, 5_000)
   - Checks require_sponsor == true
   - Reads Sponsor(candidate) → member1 ✅
   - Stores Member.sponsored_by = member1 (permanent audit trail)
   - Deletes temporary Sponsor(candidate) storage key
   - Proceeds with normal join flow

Without sponsor:
   - join() returns Error::SponsorRequired ❌
```

---

### Example 11: UpdateConfig Proposal

```
=== Members Want to Increase Contribution Amount ===

1. Alice proposes:
   propose(alice, UpdateConfig(new_config))
   - new_config.contribution_amount = 2,000 (was 1,000)
   - new_config.token_address = same as before (must match!)
   - Voting period: 7 days
   - Cooldown: 7 days after voting

2. Members vote over 7 days:
   - Alice votes For (weight: 12)
   - Bob votes For (weight: 14)
   - Carol votes Against (weight: 13)
   - Total weight: 50 (all active members)
   - For: 26, Against: 13
   - 26 × 100 > 50 × 50 → passes ✅

3. 7-day cooldown period (safety buffer)

4. After 14 days total, any member calls execute_proposal:
   - Validates new config
   - Checks token_address unchanged
   - Replaces stored config
   - Proposal marked executed

5. Next round uses new contribution_amount = 2,000
```

---

### Example 12: Two-Step Exit

```
=== Alice Wants to Leave ===

Round 10, Alice's net_balance = 1,200 (positive)

1. Alice calls request_exit(alice)
   - net_balance >= 0 ✅
   - status: Active → ExitPending
   - active_members-- (decremented immediately)

2. Round 10 continues:
   - Alice is NOT in expected_contributors (ExitPending excluded)
   - Alice does NOT receive violation for not contributing
   - Alice cannot receive payout (can_receive requires Active)

3. settle_round() runs:
   - Normal settlement for all Active/Observing members
   - Round advances to 11
   - ExitPending processing (pro-rata safe):
     - Alice.claim = deposit (5,000) + net_balance (1,200) = 6,200
     - If contract balance sufficient: refund = 6,200
     - Transfer refund to Alice
     - Remove Alice from MembersList
     - Delete Alice's member data
     - (active_members already decremented in step 1)

4. Alice is fully exited ✅
```

---

### Example 13: Emergency Payout

```
=== Bob Needs Emergency Funds ===

Bob has net_balance = 3,000 (contributed more than received)

1. Bob proposes:
   propose(bob, EmergencyPayout({ requester: bob, amount: 2,000 }))
   - Voting period: 48 hours

2. Members vote within 48 hours:
   - 4 members vote For (total weight: 40)
   - 1 member votes Against (weight: 8)
   - Total active weight: 60
   - 40 × 100 > 60 × 66 → passes (66.7% > 66%) ✅

3. Any member calls execute_proposal:
   - Verifies bob.net_balance() >= 2,000 ✅
   - Transfers 2,000 from contract to Bob
   - Bob.total_received += 2,000
   - Bob.receive_count += 1
   - Sets cooldown period
   - Stats.total_paid_out += 2,000

4. Bob receives emergency funds without waiting for normal payout cycle ✅
```

---

## 8. Edge Case Handling

### 8.1 Insurance Pool Depleted

```
Scenario: Many consecutive rounds with widespread violations; insurance pool reaches zero

Handling:
1. Continue operating, but beneficiary bears more loss
2. Subsequent violations:
   - Deposit compensation (if available)
   - Insurance pool: 0 (no compensation possible)
   - Beneficiary bears the remainder (capped at max_beneficiary_loss_rate)

Recovery:
- As new contributions come in (2% insurance fee)
- Violators' deposit deductions flow through compensation
- Pool gradually recovers
```

### 8.2 Beneficiary Maximum Loss Protection

**Problem:** In extreme cases (multiple violations + insufficient deposits + empty insurance pool), the beneficiary may suffer excessive losses

**Solution:** `max_beneficiary_loss_rate` (e.g., 10%)

```
Extreme case:
- Expected payout: 9,800
- 3 violations, all with depleted deposits
- Insurance pool: 0
- Remaining shortfall: 2,940 (30%)

Handling:
- beneficiary_loss capped at max_beneficiary_loss_rate
- Recipient receives reduced but bounded payout
- Loss recorded in Round data for transparency
```

### 8.3 Handling Funds When No One Receives

```
Why would no one receive?
1. Everyone is in the observation period (new ROSCA)
2. Everyone has a score ≤ 0 (all repaying debt)
3. Everyone is in cooldown period (rare)

Handling:
1. Insurance fee still collected (contribution × insurance_rate)
2. Remaining amount (contribution - insurance) refunded to each contributor
3. Round recorded with recipient = None, payout_amount = 0
4. Round still advances
```

### 8.4 Deposit Falls Below Minimum

```
Scenario: Alice has violated multiple times; deposit has dropped to 1,000 < 3,000 (min_deposit)

Handling:
1. Alice can top up via top_up_deposit(alice, 2,000)
   - deposit: 1,000 → 3,000 ✅
   - Validates: deposit + amount <= max_deposit

2. If not topped up:
   - Cannot receive (no explicit check, but violation lockout/low score prevents it)
   - Can continue contributing (repaying debt)
   - Can request exit (if net_balance >= 0)
   - Refund is only the remaining deposit

3. If deposit reaches 0 and violation_count >= max_violations:
   - Forcibly kicked out (status = Kicked)
```

### 8.5 Member Count Falls Very Low

```
Scenario: Many members exit; only 2 remain

Current behavior:
- Contract continues operating
- 2 members take turns receiving
- Effectively becomes a 1v1 mutual aid

Recommendation:
- Set a minimum member count restriction in config
- If below threshold: trigger dissolution vote
- Or add automatic dissolution when count < N
```

### 8.6 ExitPending Member Protection

```
Scenario: Alice set ExitPending, but settle_round hasn't run yet

Protection:
- active_members decremented immediately in request_exit() (not at settle time)
- ExitPending members do NOT appear in expected_contributors
- They receive NO violation for not contributing
- They CANNOT receive payouts (can_receive requires Active)
- They are processed at the END of settle_round, after normal operations
- Their refund = deposit + max(0, net_balance), pro-rata scaled if contract balance insufficient
```

### 8.7 Token Address Immutability

```
Scenario: Malicious UpdateConfig proposal tries to change token_address

Protection:
- execute_proposal explicitly checks: new_config.token_address != config.token_address → Error
- This prevents:
  - Redirecting funds to a different token contract
  - Draining funds via token swap
  - Any form of fund theft via config manipulation
```

---

## 9. Error Codes

```rust
enum Error {
    // Initialization errors
    AlreadyInitialized = 1,
    NotInitialized = 2,

    // Permission errors
    Unauthorized = 10,
    NotAdmin = 11,

    // Member-related errors
    MemberAlreadyExists = 20,
    MemberNotFound = 21,
    MemberNotActive = 22,
    InsufficientDeposit = 23,
    CannotExit = 24,
    CannotReceive = 25,
    JoinNotAllowed = 26,
    InObservationPeriod = 27,
    InCooldownPeriod = 28,
    InViolationLockout = 29,

    // Contribution-related errors
    AlreadyContributed = 30,
    ContributionPeriodNotStarted = 31,
    ContributionPeriodEnded = 32,
    InvalidContributionAmount = 33,
    GracePeriodEnded = 34,

    // Payout-related errors
    RecipientAlreadySet = 40,
    NoEligibleRecipient = 41,
    InsufficientFunds = 42,
    NegativePriorityScore = 43,
    InsufficientNetBalance = 44,

    // Violation-related errors
    MaxViolationsReached = 50,
    MaxLateCountReached = 51,

    // Configuration errors
    InvalidConfig = 60,
    InvalidPeriod = 61,
    InvalidDepositRange = 62,

    // ROSCA status errors
    RoscaNotActive = 70,
    RoscaPaused = 71,
    RoscaDissolved = 72,

    // Voting errors
    ProposalNotFound = 80,
    ProposalAlreadyExecuted = 81,
    VotingPeriodNotEnded = 82,
    VotingPeriodEnded = 83,
    InsufficientVotes = 84,
    AlreadyVoted = 85,
    CooldownNotEnded = 86,
    InvalidProposalType = 87,
    SponsorRequired = 88,
    NotSponsor = 89,

    // Round errors
    RoundNotEnded = 90,
    RoundAlreadySettled = 91,

    // Others
    Overflow = 100,
    InvalidState = 101,
}
```

---

## 10. Technical Considerations

### 10.1 Time Management

Using Soroban's ledger timestamp:
```rust
let current_time = env.ledger().timestamp();
```

Round calculation:
```rust
let round_start = start_time + (current_round * contribution_period);
let round_end = round_start + contribution_period;
```

### 10.2 Automated Triggering

`settle_round()` is **permissionless** — anyone can call it after the round ends + grace period.

**Recommended approach:** Backend scheduled task (cron every 30 seconds)
```javascript
// Check if round has ended + grace period → call settle_round with random seed
async function settleIfNeeded() {
    const currentTime = Date.now() / 1000;
    const roundEnd = startTime + (currentRound * contributionPeriod) + contributionPeriod;
    const settleAfter = roundEnd + violationGracePeriod;
    if (currentTime >= settleAfter) {
        const seed = BigInt(currentTime) ^ BigInt(latestLedger);
        await contract.settle_round({ random_seed: seed });
    }
}
```

### 10.3 Random Seed

`settle_round` accepts a `random_seed` parameter for weighted random recipient selection.

**Recommended sources (ordered by security):**
```javascript
// 1. Multi-source combination (recommended)
const seed = BigInt(timestamp) ^ BigInt(blockHash) ^ BigInt(nonce);

// 2. Timestamp + ledger information
const seed = Date.now() + latestLedger;

// 3. External VRF service (most secure but costly)
const seed = await chainlinkVRF.getRandomNumber();
```

**Not recommended:**
- ❌ Simple incrementing sequence (predictable)
- ❌ Fully controlled by a single party (manipulable)

### 10.4 Gas Optimization

- Batch process violations in settle_round (single transaction for all violators)
- ExitPending members processed in the same settle_round call
- `calculate_recipient()` is a read-only query (no gas on chain)

### 10.5 Storage Optimization

All data stored in `instance` storage:
- Config, state, members, rounds, statistics, governance
- Storage keys are typed enums for type safety
- Member data is per-address (no array scanning for individual lookups)

### 10.6 Security

**Reentrancy protection:**
- Soroban SDK provides atomicity guarantees
- State updates occur before token transfers

**Integer overflow:**
- Use Rust's default overflow checks in debug mode
- Amount range validation in `validate_config`

**Authorization:**
- All member-facing functions require `member.require_auth()`
- Governance functions require the caller to be an active member

**Token address immutability:**
- `UpdateConfig` explicitly rejects changes to `token_address`

---

## 11. Deployment

### 11.1 Build and Deploy

```bash
# Build the contract
cargo build --target wasm32-unknown-unknown --release

# Optimize WASM
soroban contract optimize \
  --wasm target/wasm32-unknown-unknown/release/rosca_v2.wasm

# Deploy
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/rosca_v2.wasm \
  --source DEPLOYER_SECRET_KEY \
  --network testnet

# Initialize
soroban contract invoke \
  --id CONTRACT_ID \
  --source DEPLOYER_SECRET_KEY \
  --network testnet \
  -- initialize \
  --config '{ ... }'
```

### 11.2 Testing

```bash
# Run all tests
cargo test --package rosca_v2

# Current test suite (22 tests):
# - test_initialize
# - test_join
# - test_priority_score
# - test_can_receive_conditions
# - test_settle_round_with_violation
# - test_weighted_random_selection
# - test_sponsor_flow
# - test_sponsor_required_without_sponsor_fails
# - test_top_up_deposit
# - test_request_exit_and_settle
# - test_time_based_cooldown
# - test_update_config_proposal
# - test_contribute_late_duplicate_rejected
# - test_contribute_late_exit_pending_rejected
# - test_kicked_member_deposit_confiscated
# - test_observing_member_stats_tracking
# - test_top_up_deposit_kicked_rejected
# - test_update_config_cannot_change_status
# - test_validate_config_rejects_invalid
# - test_insurance_pool_not_double_counted
# - test_settle_round_blocked_during_grace_period
# - test_exit_refund_capped_at_contract_balance
```

---

## 12. Recommended Parameter Configurations

### Small-Scale Mutual Aid (1,000 USDC)

```rust
RoscaConfig {
    contribution_amount: 1_000_000_000,    // 1,000 USDC (7 decimal places)
    contribution_period: 604_800,          // 7 days

    min_deposit: 3_000_000_000,
    recommended_deposit: 5_000_000_000,
    max_deposit: Some(10_000_000_000),

    insurance_rate: 2,                     // 2%
    max_insurance_pool: 20_000_000_000,
    max_insurance_coverage: 2_000_000_000,

    observation_contributions: 3,
    all_members_observation: true,

    cooldown_type: DynamicMembers,

    violation_grace_period: 86_400,        // 24 hours
    violation_penalties: vec![
        ViolationPenalty { deposit_deduction: 1_000_000_000, points_deduction: 3, lockout_rounds: 2 },
        ViolationPenalty { deposit_deduction: 2_000_000_000, points_deduction: 5, lockout_rounds: 5 },
    ],
    max_violations: 3,

    late_fee_rates: vec![5, 10, 20],       // 5%, 10%, 20%
    max_late_count: 3,

    max_beneficiary_loss_rate: 10,         // 10%

    allow_join: true,
    require_sponsor: false,
    status: RoscaStatus::Active,
    token_address: USDC_CONTRACT_ADDRESS,
}
```

### Large-Scale Mutual Aid (10,000 USDC)

```rust
RoscaConfig {
    contribution_amount: 10_000_000_000,
    contribution_period: 2_592_000,        // 30 days

    min_deposit: 30_000_000_000,
    recommended_deposit: 50_000_000_000,
    max_deposit: Some(100_000_000_000),

    insurance_rate: 3,                     // 3% (higher risk pool)
    max_insurance_pool: 200_000_000_000,
    max_insurance_coverage: 30_000_000_000,

    observation_contributions: 5,          // longer observation
    all_members_observation: true,

    cooldown_type: TimeBased(15_552_000),  // 180 days

    violation_grace_period: 259_200,       // 3 days
    violation_penalties: vec![
        ViolationPenalty { deposit_deduction: 15_000_000_000, points_deduction: 5, lockout_rounds: 3 },
        ViolationPenalty { deposit_deduction: 30_000_000_000, points_deduction: 10, lockout_rounds: 7 },
    ],
    max_violations: 2,                     // stricter (kicked after 2 violations)

    late_fee_rates: vec![10, 20, 30],      // 10%, 20%, 30%
    max_late_count: 3,

    max_beneficiary_loss_rate: 5,          // 5% (stricter)

    allow_join: true,
    require_sponsor: true,                 // sponsor required for large amounts
    status: RoscaStatus::Active,
    token_address: USDC_CONTRACT_ADDRESS,
}
```

---

## 13. Summary

### 13.1 Core Advantages

1. **Fair and Transparent**: Dual tracking system (amount + points); everyone's contributions and receipts are balanced
2. **Controlled Risk**: Three-layer guarantee mechanism reduces the impact of defaults
3. **Flexible Participation**: Dynamic joining/exiting with no fixed end time
4. **Incentive-Compatible**: Violators bear consequences (deposit + points + lockout); compliant members are protected and rewarded (on-time streak discounts)
5. **Decentralized Governance**: No admin with unilateral control; all changes require voting
6. **Weighted Fairness**: Higher contributors have more voting weight and higher payout probability

### 13.2 Key Innovations

1. **Dual Tracking System**: Debt (amount) + Points (count) — ensures both monetary and behavioral fairness
2. **Dynamic Membership**: Breaks the traditional ROSCA fixed-membership constraint
3. **Insurance Pool**: Community risk-sharing fund, funded by a small percentage of each contribution
4. **Observation Period**: Guards against new-member misconduct (prove commitment first)
5. **Progressive Penalties**: Escalating consequences — deposit deduction + points reduction + time lockout
6. **On-Time Streak Rewards**: Consistent contributors get discounts on occasional late fees
7. **Weighted Random Selection**: Priority-based but probabilistic — fair yet not deterministic
8. **Two-Step Exit**: Clean separation of intent and execution — no mid-round disruptions
9. **Governance Proposals**: Democratic control over config changes, emergency payouts, and dissolution
10. **Sponsor System**: Social accountability layer for membership quality control
11. **Token Address Immutability**: Hard-coded protection against fund theft via config manipulation

### 13.3 File Structure

```
contracts/rosca_v2/src/
├── lib.rs        — Main contract implementation (all public functions + helpers)
├── types.rs      — Data structures (RoscaConfig, Member, Round, Proposal, etc.)
├── storage.rs    — Storage key definitions (DataKey enum)
├── errors.rs     — Error code definitions
└── test.rs       — Unit tests (22 tests covering all features and edge cases)
```

---

**Last Updated:** 2026-03-13

**v2.1 Update Notes:**
- ✅ Insurance pool mechanism: 2% rate + surplus refund mechanism
- ✅ Violation penalties: Changed to progressive (1st time -3 points, 2nd time -5 points, 3rd time kicked out)
- ✅ Late fees: Changed to progressive (5% → 10% → 20%)
- ✅ Beneficiary protection: Set maximum loss rate at 10%
- ✅ No-recipient handling: Refund contributions to current round's contributors
- ✅ Cooldown period: Retained dynamic cooldown period mechanism
- ✅ Added `late_count` field to track late contribution count
- ✅ Added `max_beneficiary_loss_rate` protection parameter

**v2.2 Update Notes (2026-03-13):**
- ✅ `settle_round` now waits for grace period (prevents preempting `contribute_late`)
- ✅ ExitPending refund uses pro-rata distribution (prevents panic when claims exceed contract balance)
- ✅ Dissolution refund uses pro-rata distribution (same protection)
- ✅ EmergencyPayout validates `amount > 0` and checks contract token balance before transfer
- ✅ UpdateConfig preserves `status` from current config (prevents bypassing Dissolution governance)
- ✅ `validate_config` expanded: insurance_rate ≤ 100, max_beneficiary_loss_rate ≤ 100, max_deposit ≥ min_deposit, violation_penalties non-empty
- ✅ Voting threshold checks cast to u64 to prevent u32 overflow
- ✅ `final_payout` clamped to `max(0, ...)` to prevent negative payouts
- ✅ Kicked members have ALL remaining deposit confiscated (deposit → 0)
- ✅ Kicked Observing members don't affect `active_members` count
- ✅ `active_members` decremented immediately in `request_exit()`, not at settle time
- ✅ Insurance pool capped at `max_insurance_pool` in `contribute()` and `contribute_late()`
- ✅ Insurance pool no longer double-counted in `settle_round` (was added in contribute AND settle)
- ✅ `contribute_late` checks member status (Active/Observing only) and duplicate contribution
- ✅ `top_up_deposit` rejects Kicked/ExitPending members
- ✅ `Member.sponsored_by: Option<Address>` added for sponsor audit trail
- ✅ `join()` cleans up temporary `Sponsor(candidate)` storage key after reading
- ✅ Test suite expanded from 12 to 22 tests

**v2.0 Update Notes:**
- ✅ Added detailed points system design (1 point = 1 contribution)
- ✅ Added automatic payout priority ranking mechanism
- ✅ Added the impact of violations on points and lockout period mechanism
- ✅ Updated member data structure with points-related fields
- ✅ Improved frontend display recommendations
