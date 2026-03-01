# ROSCA V2 Contract Design Document

## 1. Project Overview

ROSCA V2 is a decentralized mutual savings contract built on Soroban that supports dynamic member management, deposit guarantees, and default protection mechanisms.

### 1.1 Core Features

- ✅ **Deposit System**: Members must pay a deposit before joining, used for default penalties and risk coverage
- ✅ **Dual Tracking System**:
  - Debt system (amount dimension): tracks contributed and received amounts
  - Points system (count dimension): determines payout priority and eligibility
- ✅ **Dynamic Membership**: Supports joining and leaving at any time (subject to conditions)
- ✅ **Default Protection**: Three-layer guarantee mechanism (defaulter's deposit → insurance pool → beneficiary bears loss)
- ✅ **Smart Priority**: Automatically calculates payout order based on contribution count and violation history
- ✅ **Observation Period**: New members must complete an observation period before receiving payouts
- ✅ **Flexible Configuration**: Admin can configure contribution amount, cycle, deposit requirements, and other parameters

### 1.2 Design Principles

1. **Simplicity First**: Avoid complex cross-member cost-sharing; use the beneficiary-bears-loss model
2. **Fair and Transparent**: Each person's contributions and receipts are balanced; debt must be cleared before exiting
3. **Controlled Risk**: Multi-layer guarantee mechanism to reduce the impact of defaults
4. **Flexible Participation**: Supports dynamic joining/exiting with no fixed end time

---

## 2. Core Mechanisms

### 2.1 Debt System (Amount Dimension)

Each member maintains a debt balance:

```
net_balance = total_contributed - total_received
```

- `total_contributed`: cumulative amount contributed
- `total_received`: cumulative amount received

**Key Rules:**
- `net_balance >= 0`: member may exit
- `net_balance < 0`: member must continue contributing to repay the debt

**Purpose:**
- Ensures each person's contributions and receipts are balanced
- Used as the criterion for determining exit eligibility
- Complements the points system as a fairness guarantee at the monetary level

### 2.2 Points System (Count Dimension)

#### 2.2.1 Points Calculation Formula

```
priority_score = contribution_count - (receive_count × members_count) - (violation_count × 3)
```

**Parameter Description:**
- `contribution_count`: number of contributions (+1 point each)
- `receive_count`: number of payouts received (-N points each, where N = member count at time of receipt)
- `violation_count`: number of violations (-3 points each, additional penalty)

**Core Principle: 1 point = 1 contribution**

#### 2.2.2 Points Rules

```
Contribute once (on time or within grace period): +1 point
Receive once (10-member pool): -10 points
Violate once: -3 points (additional penalty)
```

**Example (10-member ROSCA):**
```
Alice:
  - Contributed 12 times: +12
  - Received 1 time: -10
  - Violations 0: 0
  Score = 12 - 10 - 0 = 2 ✅ Eligible to receive

Bob:
  - Contributed 11 times: +11
  - Received 1 time: -10
  - Violations 1: -3
  Score = 11 - 10 - 3 = -2 ❌ Not eligible (needs to contribute 3 more times)
```

#### 2.2.3 Role of Points

1. **Payout eligibility check**: `priority_score > 0` is required to request a payout
2. **Payout priority ranking**: Higher score = higher priority
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

#### 2.2.5 Impact of Violations on Points

| Violations | Points Penalty | Deposit Deduction | Lockout Period | Status |
|-----------|---------------|-------------------|----------------|--------|
| 1 time | -3 points | -1,000 | 2 rounds | Warning |
| 2 times | -6 points | -2,000 | 5 rounds | Serious |
| 3 times | -9 points | -3,000 | - | Kicked Out ❌ |

**Combined Effect:**
```
After Bob's 1st violation:
- Points: -3 (needs 3 additional contributions to offset)
- Deposit: deducted 1,000
- Lockout: cannot receive for 2 rounds
- Record: violation_count = 1

If Bob violates 2 more times → kicked out
```

### 2.3 Deposit Mechanism

#### 2.3.1 Deposit Range

```rust
min_deposit = contribution_amount × 3     // 3,000
recommended_deposit = contribution_amount × 5  // 5,000
max_deposit = contribution_amount × 10    // 10,000
```

- Members may freely choose a deposit amount within the range
- The deposit does not affect payout weight; it serves only as a security bond
- If the deposit falls below the minimum, the member must top it up

#### 2.3.2 Use of Deposits

1. **Violation penalty**: Deposit is deducted when a member defaults
2. **Compensation fund**: Used to compensate affected recipients
3. **Exit refund**: Fully refunded upon normal exit

### 2.4 Insurance Pool Mechanism

#### 2.4.1 Insurance Pool Sources

```
Each contribution of 1,000:
- 980 → current round contribution pool (98%)
- 20 → insurance pool (2%)
```

Other sources for the insurance pool:
- Forfeited deposits (from violators)
- Remaining deposits of expelled members
- Late fee income

#### 2.4.2 Insurance Pool Uses

- Compensate recipients (when a defaulter's deposit is insufficient)
- Buffer default risk and reduce beneficiary losses
- Ensure system stability

#### 2.4.3 Insurance Pool Cap and Refund

```rust
max_insurance_pool = contribution_amount × 20  // 20,000
```

**Insurance Pool Management:**
- When the cap is reached: new contributions go entirely into the contribution pool (the 2% deduction stops)
- When a member exits: if the insurance pool has a surplus, a portion of the insurance fees is refunded proportionally based on contribution share
- When the contract ends: the remaining insurance pool balance is distributed to all members proportionally based on contributions

**Actual user cost:**
```
Ideal case (no defaults):
- Contributed 10 times = 10,000
- Insurance fees = 200 (2%)
- Refunded on exit = 150 (75% of surplus)
- Actual cost = 50 (0.5%)

With defaults:
- Insurance fees used to compensate affected parties
- Refund ratio reduced or zero
- Actual cost ≤ 2%
```

### 2.5 Default Handling Mechanism

#### 2.5.1 Three-Layer Guarantee

When a member defaults, the current round's recipient is compensated in the following order:

```
1. Defaulter's deposit (1:1 compensation)
   ↓ insufficient
2. Insurance pool compensation (max N violations per round)
   ↓ still insufficient
3. Beneficiary bears the final loss
```

#### 2.5.2 Violation Penalties (Progressive)

**Progressive penalty mechanism:** The more violations, the heavier the penalty

```rust
1st violation:
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
- All remaining deposit deducted
- Status: Kicked Out ❌
- Cannot participate again
```

**Penalty configuration:**
```rust
struct ViolationPenalty {
    deposit_deduction: i128,    // deposit deduction amount
    points_deduction: i32,      // points deduction
    lockout_rounds: u64,        // number of lockout rounds
}

fn get_violation_penalty(violation_count: u32) -> ViolationPenalty {
    match violation_count {
        1 => ViolationPenalty {
            deposit_deduction: 1_000,
            points_deduction: 3,
            lockout_rounds: 2,
        },
        2 => ViolationPenalty {
            deposit_deduction: 2_000,
            points_deduction: 5,
            lockout_rounds: 5,
        },
        _ => ViolationPenalty::Kicked,  // kicked out on 3rd violation
    }
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

**Example:**
```
Bob's 1st violation in Round 5:
- violation_lockout_until = 5 + 2 = 7
- Round 6: even if points are sufficient, cannot receive (current_round < 7)
- Round 7: lockout ends; can receive if conditions are met

Bob's 2nd violation in Round 10:
- violation_lockout_until = 10 + 5 = 15
- Heavier points penalty (-5 instead of -3)
- Higher deposit deduction (2,000 instead of 1,000)
```

#### 2.5.4 Grace Period and Late Fees

**Grace period settings:**
```
Contribution deadline: every Monday 00:00
Grace period: 24 hours
Actual violation determination: Tuesday 00:00

Contributing within grace period:
- Does not count as a violation ✅
- Must pay a late fee (progressive)
- contribution_count increases normally by +1
```

**Progressive late fees:**
```rust
struct Member {
    late_count: u32,  // cumulative number of late contributions
}

fn calculate_late_fee(late_count: u32, contribution_amount: i128) -> i128 {
    let rate = match late_count {
        0 => 5,   // 1st late: 5%
        1 => 10,  // 2nd late: 10%
        2 => 20,  // 3rd late: 20%
        _ => return Err("Exceeded maximum late count, treated as violation"),
    };
    contribution_amount * rate / 100
}

const MAX_LATE_COUNT: u32 = 3;  // more than 3 late contributions → treated as violation
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
- Directly treated as a violation ❌
- Processed as a violation (deposit deduction + points penalty + lockout)
```

**Where late fees go:**
```
All goes into the insurance pool (strengthens insurance capacity)
```

---

## 3. Data Structure Design

### 3.1 Contract Configuration

```rust
struct RoscaConfig {
    // Basic parameters
    contribution_amount: i128,        // contribution amount per round (e.g., 1,000)
    contribution_period: u64,         // contribution cycle (in seconds, e.g., 7 days)

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
    cooldown_type: CooldownType,      // cooldown type

    // Violation configuration (progressive)
    violation_grace_period: u64,      // grace period (24 hours)
    violation_penalties: Vec<ViolationPenalty>,  // progressive penalty configuration
    max_violations: u32,              // maximum violations allowed (3)

    // Late fee configuration (progressive)
    late_fee_rates: Vec<u32>,         // late fee rates ([5, 10, 20]%)
    max_late_count: u32,              // maximum late count (3)

    // Beneficiary protection
    max_beneficiary_loss_rate: u32,   // maximum loss rate (10%)

    // Administration
    admin: Address,                   // admin
    allow_join: bool,                 // whether new members may join
}

struct ViolationPenalty {
    deposit_deduction: i128,          // deposit deduction amount
    points_deduction: u32,            // points deduction
    lockout_rounds: u64,              // number of lockout rounds
}

enum CooldownType {
    FixedRounds(u64),      // fixed number of rounds (e.g., 10 rounds)
    DynamicMembers,        // dynamic = member count at time of receipt
    TimeBased(u64),        // time-based (e.g., 60 days)
}
```

### 3.2 Member Data

```rust
struct Member {
    // Basic information
    address: Address,
    joined_at: u64,                   // join timestamp
    status: MemberStatus,

    // === Debt system (amount dimension) ===
    deposit: i128,                    // current deposit balance
    total_contributed: i128,          // cumulative amount contributed
    total_received: i128,             // cumulative amount received

    // === Points system (count dimension) ===
    contribution_count: u32,          // number of contributions
    receive_count: u32,               // number of payouts received
    violation_count: u32,             // number of violations
    late_count: u32,                  // number of late contributions

    // === Statistics ===
    observation_count: u32,           // contributions during observation period
    last_contribution_round: u64,     // last round contributed
    last_received_round: u64,         // last round received

    // === State control ===
    cooldown_until_round: u64,        // round at which cooldown period ends
    violation_lockout_until: u64,     // round at which violation lockout ends
}

enum MemberStatus {
    Observing,      // observation period
    Active,         // active
    ExitPending,    // exit requested
    Kicked,         // kicked out
}

// Computed properties
impl Member {
    // Debt balance (amount)
    fn net_balance(&self) -> i128 {
        self.total_contributed - self.total_received
    }

    // Priority score (count)
    fn priority_score(&self, members_count: u32, config: &RoscaConfig) -> i32 {
        self.contribution_count as i32
        - (self.receive_count as i32 * members_count as i32)
        - (self.violation_count as i32 * config.violation_penalty_points as i32)
    }

    // Payout eligibility (dual conditions)
    fn can_receive(&self, current_round: u64, members_count: u32, config: &RoscaConfig) -> bool {
        self.status == MemberStatus::Active &&
        self.net_balance() >= 0 &&                                      // debt cleared
        self.priority_score(members_count, config) > 0 &&              // positive score
        self.observation_count >= config.observation_contributions &&
        current_round >= self.cooldown_until_round &&                  // cooldown ended
        current_round >= self.violation_lockout_until                  // violation lockout ended
    }

    // Exit condition
    fn can_exit(&self) -> bool {
        self.net_balance() >= 0
    }

    // Payout priority (for sorting)
    fn get_priority(&self, members_count: u32, config: &RoscaConfig) -> (i32, u32, u64) {
        (
            -self.priority_score(members_count, config),  // primary: score (descending)
            -(self.contribution_count as i32),            // secondary: total contributions (descending)
            self.joined_at,                               // tertiary: join time (ascending, earlier = higher priority)
        )
    }
}
```

### 3.3 Round Data

```rust
struct Round {
    round_id: u64,
    start_time: u64,
    end_time: u64,

    // Participants
    expected_contributors: Vec<Address>,  // members expected to contribute
    actual_contributors: Vec<Address>,    // members who actually contributed
    violators: Vec<Address>,              // members who violated

    // Funds
    total_collected: i128,                // actual amount collected
    insurance_collected: i128,            // insurance fees collected
    recipient: Option<Address>,           // payout recipient
    payout_amount: i128,                  // actual payout amount

    // Compensation
    violations_loss: i128,                // loss from violations
    deposit_compensation: i128,           // compensation from deposits
    insurance_compensation: i128,         // compensation from insurance pool
    beneficiary_loss: i128,               // loss borne by the beneficiary
}
```

---

## 4. Core Flows

### 4.1 Join Flow

```
1. User calls join(deposit_amount)
   ├─ Validate: deposit_amount >= min_deposit
   ├─ Validate: allow_join == true
   ├─ Transfer deposit in
   └─ Create Member (status = Observing)

2. Observation period (automatic)
   ├─ Member contributes normally N times (e.g., 3 times)
   ├─ observation_count++
   └─ Once requirement met: status = Active

3. Becomes a full member
   └─ May apply for a payout
```

### 4.2 Contribution Flow

```
At the start of each round:
1. Calculate members expected to contribute (Active + Observing)
2. Wait for members to call contribute()
3. Deadlines:
   ├─ Normal deadline: Monday 00:00
   └─ Grace period: Tuesday 00:00

Member contribution:
1. Call contribute()
   ├─ Transfer in contribution_amount
   ├─ Allocate: 980 → contribution pool, 20 → insurance pool
   ├─ Update: total_contributed += 1000
   ├─ Update: contribution_count++
   └─ If in observation period: observation_count++

Late contribution within grace period:
1. Call contribute_late()
   ├─ Transfer in: 1000 + late_fee (50)
   └─ late_fee → insurance pool

Violation determination (Tuesday 00:00):
1. Members who have not contributed = violation
2. Execute violation processing
```

### 4.3 Payout Flow

#### Option A: First-Come-First-Served (Simple)
```
1. Member requests payout via request_payout()
   ├─ Validate: can_receive() == true
   ├─ Validate: no other recipient this round
   └─ Record: round.recipient = caller (first come, first served)

2. Settlement after round ends
   └─ Execute payment flow
```

#### Option B: Priority-Weighted Random Selection (Implemented) ✅
```
1. At round settlement (called by admin/backend)

   Step 1: Filter eligible candidates
   ├─ Iterate all members
   ├─ Filter: can_receive() == true
   └─ Obtain candidate list

   Step 2: Calculate weight for each candidate
   ├─ weight = max(priority_score, 1)
   ├─ total_weight = sum(all candidate weights)
   └─ Higher priority → higher weight

   Step 3: Weighted random selection
   ├─ Use externally provided random seed (random_seed)
   ├─ random_value = random_seed % total_weight
   └─ Select recipient based on cumulative weight

2. Payment flow

Call method:
settle_round(random_seed) // admin/backend provides random seed

Advantages:
✅ Members with higher priority have a greater chance of being selected (fair)
✅ But not 100% deterministic (gives others a chance)
✅ Avoids indefinite waiting (lower-priority members also have a small chance)

Example:
3 candidates with priorities 10, 5, 2
- member1 weight 10 → probability 10/17 ≈ 59%
- member2 weight 5  → probability 5/17  ≈ 29%
- member3 weight 2  → probability 2/17  ≈ 12%
```

#### API Design

**1. settle_round(random_seed) - Settlement function**
```rust
pub fn settle_round(env: Env, random_seed: u64) -> Result<(), Error>
```
- **Parameter**: `random_seed` is a random seed provided by the admin/backend
- **Caller**: Admin manually or backend scheduled task
- **Permission**: Requires admin authorization

**2. calculate_recipient() - Query function**
```rust
pub fn calculate_recipient(env: Env) -> Result<Option<Address>, Error>
```
- **Purpose**: Query which member currently has the highest priority (no gas cost, for reference only)
- **Returns**: The address of the member with the highest priority
- **Note**: This is a query only; actual settlement uses weighted random selection

#### Random Seed Sources

**Recommended approaches** (ordered by security):
```javascript
// 1. Multi-source combination (most recommended)
const seed = BigInt(timestamp) ^ BigInt(blockHash) ^ BigInt(nonce);

// 2. Timestamp + block information
const seed = Date.now() + block_hash;

// 3. External true-random number service (most secure but costly)
const seed = await chainlinkVRF.getRandomNumber();
```

**Not recommended**:
- ❌ Simple incrementing sequence (predictable)
- ❌ Fully controlled by admin (manipulable)

#### Backend Integration Examples

**Node.js scheduled task**:
```javascript
const { SorobanClient } = require('@stellar/stellar-sdk');
const cron = require('node-cron');

async function settleRound() {
  // 1. Generate random seed
  const timestamp = Date.now();
  const blockHash = await getLatestBlockHash();
  const randomSeed = BigInt(timestamp) ^ BigInt(blockHash);

  // 2. Call the contract
  const contract = new Contract(contractId);
  await contract.settle_round({
    random_seed: randomSeed
  });

  console.log(`Round settled with seed: ${randomSeed}`);
}

// Execute every Monday at 00:00
cron.schedule('0 0 * * 1', settleRound);
```

**Python scheduled task**:
```python
import time
import schedule
from stellar_sdk import SorobanServer, Keypair, TransactionBuilder

def settle_round():
    # Generate random seed
    timestamp = int(time.time() * 1000)
    random_seed = timestamp

    # Call the contract
    contract.invoke(
        'settle_round',
        random_seed=random_seed
    )

    print(f"Round settled with seed: {random_seed}")

# Execute every Monday at 00:00
schedule.every().monday.at("00:00").do(settle_round)

while True:
    schedule.run_pending()
    time.sleep(60)
```

### 4.4 Settlement Flow

```
When settle_round(random_seed) is executed:

   ├─ Collect contributions: total_collected
   ├─ Process violations:
   │   ├─ Deduct violator deposit + points + set lockout period
   │   ├─ Compensate from insurance pool
   │   └─ Calculate final payout amount
   ├─ Weighted random selection of recipient
   ├─ Pay the recipient
   ├─ Update member data:
   │   ├─ total_received += payout_amount
   │   ├─ receive_count++
   │   ├─ last_received_round = current_round
   │   └─ cooldown_until_round = current_round + cooldown
   └─ Record round data
```

**Priority sorting example:**
```
Round 8, 4 members meet conditions:

Alice:   priority_score = 2,  contribution_count = 12
Bob:     priority_score = 2,  contribution_count = 11
Carol:   priority_score = 1,  contribution_count = 11
Dave:    priority_score = 0,  contribution_count = 10  ❌ Not eligible (score must be > 0)

Candidates: Alice, Bob, Carol

Sorting:
1. priority_score: Alice=2, Bob=2, Carol=1
   → Alice and Bob are tied
2. contribution_count: Alice=12 > Bob=11
   → Alice wins

Result: Alice receives this round ✅
```

### 4.4 Exit Flow

```
1. Member requests exit via request_exit()
   ├─ Validate: net_balance() >= 0
   ├─ Validate: no outstanding contribution obligations
   └─ Status: status = ExitPending

2. At the start of the next round (or immediately)
   ├─ Calculate refund:
   │   ├─ Deposit: deposit
   │   └─ Net contribution: max(0, net_balance())
   ├─ Transfer out refund
   └─ Delete member record
```

### 4.5 Violation Processing Flow

```
At the end of a round where Bob has violated:

1. Calculate loss
   violation_loss = contribution_amount = 1,000

2. Penalize the violator Bob
   ├─ Deduct deposit: Bob.deposit -= violation_penalty (1,000)
   ├─ Deduct points: Bob.violation_count++ (affects priority_score by -3)
   ├─ Set lockout period:
   │   ├─ 1st violation: lockout_until = current_round + 2
   │   ├─ 2nd violation: lockout_until = current_round + 5
   │   └─ 3rd violation: lockout_until = current_round + 10
   └─ Check whether to kick out:
       └─ If violation_count >= 3 or deposit <= 0: status = Kicked

3. Compensate the recipient (Carol)

   Step 1: From Bob's deposit
   ├─ available = min(1,000, amount deducted from Bob)
   ├─ compensation_from_deposit = available
   └─ Remaining shortfall = 1,000 - available

   Step 2: From insurance pool (if shortfall remains)
   ├─ available = min(shortfall, insurance_pool, max_coverage_per_round)
   ├─ compensation_from_insurance = available
   └─ Remaining shortfall -= available

   Step 3: Beneficiary bears the loss
   ├─ beneficiary_loss = remaining shortfall
   └─ Carol.total_received = actual amount (less than expected)

4. Deposit enters the insurance pool
   └─ insurance_pool += compensation_from_deposit

5. Bob's subsequent status
   ├─ If not kicked out:
   │   ├─ May continue contributing (repaying debt)
   │   ├─ But cannot receive during lockout period
   │   └─ Points reduced, payout priority lowered
   └─ If kicked out:
       └─ Cannot participate in any further operations
```

**Example: Bob's 1st violation**
```
Before violation:
  deposit: 5,000
  contribution_count: 10
  receive_count: 1
  violation_count: 0
  priority_score: 10 - 10 - 0 = 0

After violation:
  deposit: 4,000 (deducted 1,000)
  violation_count: 1
  violation_lockout_until: current_round + 2
  priority_score: 10 - 10 - 3 = -3 ❌ (cannot receive; needs 4 more contributions)

Requirements:
  - Contribute 4 more times to get priority_score > 0
  - Wait for 2-round lockout to end
  - Both conditions must be met to receive again
```

---

## 5. Detailed Examples

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
  - priority_score: 3 - 0 - 0 = 3
  - net_balance: 3,000

=== Round 4 ===
Alice meets payout conditions (assume highest priority):
  - priority_score: 3 > 0 ✅
  - net_balance: 3,000 >= 0 ✅
  - Observation period passed ✅
  - Automatically selected as this round's recipient

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
  - priority_score: 10 - 10 - 0 = 0 ❌ (debt just cleared, but must be > 0)
  - net_balance: 10,000 - 9,800 = 200 ✅

=== Round 11 ===
Alice contributes once more:
  - contribution_count: 11
  - priority_score: 11 - 10 - 0 = 1 ✅
  - net_balance: 1,200
  - Can receive again or exit

=== Round 12 ===
Alice requests exit:
  - priority_score: 1 > 0 ✅ (eligible to receive, but chooses to exit)
  - net_balance: 1,200 >= 0 ✅
  - Refund:
    - Deposit: 5,000
    - Net contribution: 1,200
    - Total: 6,200
  - Exit successful

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
- Round 5, Carol requests a payout
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
   violation_loss = 980 (missing contribution)

2. Deduct Bob's deposit:
   - Bob.deposit: 5,000 → 4,000
   - Bob.violation_count: 0 → 1
   - Amount deducted: 1,000

3. Compensate Carol:
   Step 1: From Bob's deposit
     - Needed: 980
     - Available: 1,000 ✅
     - Compensation: 980
     - Remaining: 20 → insurance pool

   Step 2: Insurance pool compensation
     - Not needed ✅

   Step 3: Carol bears loss
     - 0 ✅

4. Final result:
   - Carol receives: 9,800 (full amount) ✅
   - Carol.total_received: 9,800
   - Bob.deposit: 4,000 (deducted 1,000)
   - Insurance pool: +20
```

---

### Example 3: Violation Scenario — Single Violation, Deposit Insufficient

**Background:**
- Bob has violated twice; only 600 deposit remaining
- Violates again in Round 8

```
=== Round 8 ===
Actual contributions:
  - 9 members contribute: 8,820
  - Bob defaults: 0

=== Violation Processing ===

1. Calculate loss: 980

2. Deduct Bob's deposit:
   - Bob.deposit: 600 → 0
   - Bob.violation_count: 2 → 3 (limit reached)
   - Amount deducted: 600 (all of it)
   - Status: status = Kicked ❌

3. Compensate Carol:
   Step 1: From Bob's deposit
     - Needed: 980
     - Available: 600
     - Compensation: 600
     - Remaining shortfall: 380

   Step 2: Insurance pool compensation
     - Insurance pool balance: 1,500
     - Needed: 380
     - Compensation: 380 ✅
     - Insurance pool: 1,500 → 1,120
     - Remaining shortfall: 0

   Step 3: Carol bears loss
     - 0 ✅

4. Final result:
   - Carol receives: 9,800 (full amount) ✅
   - Bob is kicked out; deposit reduced to zero
   - Insurance pool: +600 (Bob's deposit) -380 (compensation) = +220
```

---

### Example 4: Violation Scenario — Multiple Violations, Beneficiary Bears Partial Loss

**Background:**
- Round 10
- 3 members violate simultaneously (Alice, Bob, Charlie)
- Insurance pool balance: 1,000

```
=== Round 10 ===
Actual contributions:
  - 7 members contribute: 7 × 980 = 6,860
  - 3 members default: 0
  - Total shortfall: 3 × 980 = 2,940

=== Violation Processing ===

1. Deduct deposits from 3 members:
   - Alice.deposit: 5,000 → 4,000 (deducted 1,000)
   - Bob.deposit: 3,500 → 2,500 (deducted 1,000)
   - Charlie.deposit: 4,000 → 3,000 (deducted 1,000)
   - Total deducted: 3,000

2. Compensate Dave (recipient):
   Step 1: From violators' deposits
     - Needed: 2,940
     - Available: 3,000 ✅
     - Compensation: 2,940
     - Remaining: 60 → insurance pool

   Step 2: Insurance pool compensation
     - Not needed ✅

   Step 3: Dave bears loss
     - 0 ✅

3. Final result:
   - Dave receives: 9,800 (full amount) ✅
   - Each of the 3 members has 1,000 deducted from their deposit
   - Insurance pool: +60
```

**Extreme case (if deposits are also insufficient):**

Assume all 3 members have only 200 deposit remaining:

```
1. Deduct deposits:
   - 3 members total: 600

2. Compensate Dave:
   Step 1: From deposits
     - Needed: 2,940
     - Available: 600
     - Compensation: 600
     - Remaining shortfall: 2,340

   Step 2: Insurance pool
     - Available: 1,000
     - Maximum compensation per round: 2,000 ✅
     - Compensation: 1,000
     - Remaining shortfall: 1,340

   Step 3: Dave bears loss
     - beneficiary_loss: 1,340 ❌

3. Final result:
   - Dave receives: 9,800 - 1,340 = 8,460
   - Dave.total_received: 8,460 (actual amount recorded)
   - Dave.net_balance calculated based on 8,460
   - All 3 members are kicked out (deposits reduced to zero)
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
Eve and Frank exit (already received, debt cleared):
  - 8 members remaining
  - Collected per round: 8 × 980 = 7,840

=== Round 5 ===
Grace receives:
  - 8 members contribute: 7,840
  - Grace receives: 7,840 ✅
  - Grace.total_received: 7,840
  - Grace needs to repay: 7,840 (8 contributions)

=== Round 8 ===
3 new members join (Helen, Iris, Jack):
  - Status: Observing
  - Total members: 11 (8 existing + 3 new)

Round 9:
  - New members must also contribute (observation period)
  - Collected per round: 11 × 980 = 10,780
  - Recipient receives: 10,780

Round 11:
  - New members pass observation period (3 contributions)
  - May apply for a payout

=== Results ===
Payout amounts differ across cohorts:
  - Rounds 1–4: 10 members → received 9,800
  - Rounds 5–7: 8 members → received 7,840
  - Round 8+: 11 members → received 10,780

But everyone is treated fairly:
  - Receive as much as you repay
  - May exit when net_balance = 0
```

---

### Example 6: Cooldown Period Mechanism

**Background:**
- Dynamic cooldown period (= member count at time of receipt)
- Alice receives when there are 10 members

```
=== Round 5 ===
Alice receives:
  - Current member count: 10
  - Alice.last_received_round: 5
  - Alice.cooldown_until_round: 5 + 10 = 15
  - Alice may receive again earliest in Round 15

=== Round 10 ===
Alice wants to receive again:
  - current_round: 10 < 15 ❌
  - Must wait until Round 15

=== Round 12 ===
2 new members join; total 12 members:
  - Alice's cooldown_until_round unchanged (still 15)
  - Cooldown is locked at time of receipt; does not change with membership

=== Round 15 ===
Alice applies to receive again:
  - current_round: 15 >= 15 ✅
  - net_balance: check whether >= 0
  - If conditions are met, may receive
  - With 12 members, Alice receives 11,760
  - New cooldown_until_round: 15 + 12 = 27
```

---

### Example 7: Observation Period — All Members

**Background:**
- all_members_observation = true
- Contract just created; 5 initial members

```
=== Round 0 (at creation) ===
5 members join:
  - Alice, Bob, Carol, Dave, Eve
  - Each deposits: 5,000
  - Status: all Observing

=== Round 1 ===
All 5 contribute:
  - observation_count: 0 → 1
  - Alice tries to apply for payout ❌
    - observation_count (1) < required (3)

=== Rounds 2–3 ===
All 5 continue contributing:
  - observation_count: 3
  - Status: all → Active ✅

=== Round 4 ===
Alice applies for payout:
  - observation_count: 3 >= 3 ✅
  - net_balance: 3,000 >= 0 ✅
  - No cooldown period (first-time receipt) ✅
  - Successfully receives

Significance:
  - Prevents the creator from immediately receiving and disappearing
  - Everyone must demonstrate commitment (contribute at least 3 times)
```

---

### Example 8: Grace Period and Late Fees

**Background:**
- Contribution deadline: every Monday 00:00
- Grace period: 24 hours
- Late fee rate: 5%

```
=== Round 10 ===
Deadline: Monday 2024-03-04 00:00

Alice's situation:
  - Monday 10:00 (past deadline)
  - Alice calls contribute()
    - ❌ Past deadline

  - Alice calls contribute_late()
    - Must pay: 1,000 + 50 (late fee) = 1,050
    - Allocation:
      - 980 → contribution pool
      - 70 → insurance pool (20 + 50)
    - ✅ Does not count as a violation
    - total_contributed += 1,000 (late fee not counted as contribution)

  - Tuesday 01:00 (after grace period ends)
  - Alice wants to pay late
    - ❌ Grace period has ended
    - Treated as a violation
    - Deposit deducted by 1,000
```

---

### Example 9: Points Priority Ranking

**Background:**
- 10-member ROSCA, Round 12
- Multiple members meet payout conditions; system automatically selects the highest-priority member

```
=== Start of Round 12 ===

Member status:

Alice (model member):
  - contribution_count: 12, receive_count: 1, violation_count: 0
  - priority_score: 12 - (1×10) - 0 = 2
  - net_balance: 2,000 ✅
  - cooldown: passed ✅
  - observation period: passed ✅

Bob (occasional violations):
  - contribution_count: 14, receive_count: 1, violation_count: 1
  - priority_score: 14 - 10 - 3 = 1
  - net_balance: 4,000 ✅
  - cooldown: passed ✅
  - violation_lockout: passed ✅

Carol (frequently late):
  - contribution_count: 13, receive_count: 1, violation_count: 0
  - priority_score: 13 - 10 - 0 = 3 ⭐ Highest
  - net_balance: 3,000 ✅
  - cooldown: passed ✅

Dave (multiple violations):
  - contribution_count: 15, receive_count: 1, violation_count: 2
  - priority_score: 15 - 10 - 6 = -1 ❌
  - Still in violation lockout period ❌
  - Cannot receive

Eve (observation period):
  - contribution_count: 2, receive_count: 0, violation_count: 0
  - priority_score: 2 - 0 - 0 = 2
  - observation_count: 2 < 3 ❌
  - Cannot receive

=== System automatically selects recipient ===

Step 1: Filter eligible candidates
  Eligible: Alice, Bob, Carol
  Ineligible: Dave (negative score + lockout), Eve (observation period not completed)

Step 2: Sort by priority
  Sorting key: (priority_score, contribution_count, joined_at)

  Carol:  (3, 13, timestamp_110)
  Alice:  (2, 12, timestamp_100)
  Bob:    (1, 14, timestamp_120)

  Primary comparison priority_score: Carol=3 > Alice=2 > Bob=1

Step 3: Select top-ranked
  This round's recipient: Carol ✅

=== Round ends ===

Carol receives:
  - 10 members contributed; 9,800 collected
  - Carol.total_received = 9,800
  - Carol.receive_count = 2
  - Carol.priority_score = 13 - (2×10) - 0 = -7 (needs to repay debt)
  - Carol.cooldown_until_round = 12 + 10 = 22

Other members' scores unchanged (did not receive this round)

=== Next Round (Round 13) ===

After Alice contributes:
  - priority_score: 13 - 10 - 0 = 3 (highest)
  - Likely to be this round's recipient

After Bob contributes:
  - priority_score: 15 - 10 - 3 = 2
  - May receive if no one has a higher score

Carol:
  - priority_score: -7 ❌
  - Needs 8 more contributions to return to a positive score
  - In cooldown period (cannot receive before Round 22)
```

**Key points:**
1. ✅ Violations affect priority: Bob contributed more but has lower points due to violations
2. ✅ Automatic ranking is fair: Carol has the highest score and automatically gets the payout opportunity
3. ✅ Negative score locks out: Dave cannot receive; must first clear his debt
4. ✅ Observation period protection: Eve, a new member, must prove commitment before receiving

---

## 6. Edge Case Handling

### 6.1 Insurance Pool Depleted

```
Scenario: Many consecutive rounds with widespread violations; insurance pool reaches zero

Handling:
1. Continue operating, but beneficiary bears more loss
2. Insurance pool balance: 0
3. Subsequent violations:
   - Deposit compensation
   - Insurance pool: 0 (no compensation possible)
   - Beneficiary bears the remainder

Recovery:
- As new contributions come in (2% injected)
- Violators' deposits forfeited
- Insurance pool gradually recovers
```

### 6.2 Beneficiary Maximum Loss Protection

**Problem:** In extreme cases (multiple violations + insufficient deposits + empty insurance pool), the beneficiary may suffer excessive losses

**Solution:** Set a maximum loss rate of 10%

```rust
struct RoscaConfig {
    max_beneficiary_loss_rate: u32,  // 10 (= 10%)
}

// Calculated at payout time
let expected_payout = members_count × 980;
let actual_collected = actual_contributors × 980;
let deficit = expected_payout - actual_collected;

// Compensation flow
let compensation_from_deposit = min(deficit, total violators' deposits);
let compensation_from_insurance = min(remaining shortfall, insurance_pool);
let final_deficit = deficit - compensation_from_deposit - compensation_from_insurance;

// Check loss rate
let loss_rate = (final_deficit × 100) / expected_payout;

if loss_rate > max_beneficiary_loss_rate {
    // Option A: Pause this round's payout
    pause_payout();
    notify_user("Too many violations this round; payout paused; funds rolled into next round");

    // Option B: Partial payment + record debt
    let actual_payout = expected_payout - final_deficit;
    pay_to_recipient(actual_payout);
    record_deficit(recipient, final_deficit);  // prioritized for compensation

    // Option C: Let the user decide
    prompt_user("Loss {loss_rate}%, do you accept?");
}
```

**Example:**
```
10-member pool, 3 violations:
- Expected: 9,800
- Actual collected: 6,860
- Shortfall: 2,940 (30%)

Compensation:
- Violators' deposits: 3,000
- Insurance pool: 0 (depleted)
- Remaining shortfall: 0 ✅ (deposits sufficient)

Extreme case (deposits also insufficient):
- Violators' deposits: 600 (3 members each with 200 remaining)
- Insurance pool: 500
- Remaining shortfall: 1,840 (18.8%) > 10% ❌

Handling:
- Pause payout, or
- With user confirmation, partial payment: 9,800 - 1,840 = 7,960
```

### 6.3 Handling Funds When No One Receives

**Scenario analysis:**
```
Why would no one receive?
1. Everyone is in the observation period (new ROSCA)
2. Everyone has a score ≤ 0 (all repaying debt)
3. Everyone is in a cooldown period (theoretically unlikely)
```

**Handling approach:**
```rust
fn handle_no_recipient(env: &Env, round: &Round) {
    let reason = determine_reason();

    match reason {
        NoRecipientReason::AllInObservation => {
            // Observation period: refund this round's contributions
            refund_to_contributors(round);
        },

        NoRecipientReason::AllInDebt => {
            // All in debt: refund contributions
            refund_to_contributors(round);
        },

        NoRecipientReason::AllInCooldown => {
            // Cooldown period: refund contributions (should not happen)
            refund_to_contributors(round);
        },
    }
}

fn refund_to_contributors(round: &Round) {
    // Refund 980 per person to this round's contributors
    // 20 per person (insurance fee) stays in the insurance pool
    for contributor in round.contributors {
        transfer(contributor, 980);
    }
}
```

**Example:**
```
Rounds 1–3, everyone in observation period:
- Round 1: 10 members contribute 10,000
  - No one can receive
  - Refunded: 980 each, total 9,800
  - Insurance pool: 200

- Round 4: Alice passes observation period
  - Alice receives: 9,800 (normally)

Result:
- Funds do not accumulate during observation period rounds
- Insurance fees collected normally
- System operates fairly
```

### 6.4 Member Count Falls to Very Low

```
Scenario: Many members exit; only 2 remain

Option A: Continue operating
  - 2 members take turns receiving
  - Collected per round: 2 × 980 = 1,960
  - Effectively becomes a 1v1 mutual aid arrangement

Option B: Auto-dissolve (recommended)
  - Set a minimum member count (e.g., 3)
  - If below threshold: trigger liquidation
  - Refund all deposits and net contributions

Recommendation: Option B (add minimum member count restriction)
```

### 6.5 Deposit Falls Below Minimum

```
Scenario: Alice has violated multiple times; deposit has dropped to 1,000 < 3,000

Handling:
1. Warning: deposit top-up required
2. Alice calls top_up_deposit()
   - Transfers in 2,000
   - deposit: 1,000 → 3,000 ✅

3. If not topped up:
   - Cannot receive (validation fails)
   - Can continue contributing (repaying debt)
   - Can exit (if net_balance >= 0)
   - But refund is only the remaining deposit (1,000)

4. If continues to violate and deposit reaches zero:
   - Forcibly kicked out
   - Debt written off (bad debt)
```

### 6.6 Admin Acting in Bad Faith

**Scenario 1: Admin closes joining**
```
- allow_join = false
- Impact: new members cannot join
- Safeguard: rules set at contract creation; admin permissions are limited
```

**Scenario 2: Admin tries to withdraw funds**
```
- This functionality should not exist
- Contract funds may only:
  1. Be paid to recipients
  2. Be refunded to exiting members
  3. Be forfeited as deposits → insurance pool
- Admin cannot transfer funds out
```

**Recommendation: Minimize admin permissions**
```rust
Admin may only:
  - Enable/disable joining (allow_join)
  - Kick out seriously violating members (requires sufficient justification)
  - Trigger liquidation (requires majority member consent)

Admin may not:
  - Modify contribution amount (fixed at contract creation)
  - Withdraw funds
  - Modify member deposits
  - Modify member debt
```

---

## 7. Contract Interface Design

### 7.1 Initialization

```rust
fn initialize(
    env: Env,
    admin: Address,
    contribution_amount: i128,
    contribution_period: u64,
    config: RoscaConfig,
) -> Result<(), Error>
```

### 7.2 Member Management

```rust
// Join
fn join(env: Env, member: Address, deposit: i128) -> Result<(), Error>

// Request exit
fn request_exit(env: Env, member: Address) -> Result<(), Error>

// Top up deposit
fn top_up_deposit(env: Env, member: Address, amount: i128) -> Result<(), Error>

// Query member information
fn get_member(env: Env, member: Address) -> Member

// Query member list
fn get_members(env: Env) -> Vec<Address>
```

### 7.3 Contributions and Payouts

```rust
// Normal contribution
fn contribute(env: Env, member: Address) -> Result<(), Error>

// Grace period contribution
fn contribute_late(env: Env, member: Address) -> Result<(), Error>

// Request payout
fn request_payout(env: Env, member: Address) -> Result<(), Error>

// Query current round
fn get_current_round(env: Env) -> Round
```

### 7.4 Administrative Functions

```rust
// Enable/disable joining
fn set_allow_join(env: Env, admin: Address, allow: bool) -> Result<(), Error>

// Trigger violation determination (called periodically or automatically)
fn process_violations(env: Env) -> Result<(), Error>

// Settle round (called periodically or automatically)
fn settle_round(env: Env) -> Result<(), Error>

// Query insurance pool
fn get_insurance_pool(env: Env) -> i128

// Query statistics
fn get_statistics(env: Env) -> Statistics

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

---

## 8. Technical Considerations

### 8.1 Time Management

Using Soroban's ledger timestamp:
```rust
let current_time = env.ledger().timestamp();
```

Cycle calculation:
```rust
fn get_current_round(env: &Env) -> u64 {
    let start_time = env.storage().instance().get(&DataKey::StartTime).unwrap();
    let period = env.storage().instance().get(&DataKey::Period).unwrap();
    (env.ledger().timestamp() - start_time) / period
}
```

### 8.2 Automated Triggering

Certain operations need to be triggered periodically:
1. Violation determination (after each round ends)
2. Round settlement (disbursing funds)

Options:
- Use off-chain scheduled tasks (recommended)
- Or check and trigger on any transaction
- Or rely on members calling in

### 8.3 Gas Optimization

- Batch process violations (handle all violators in one call)
- Paginated querying of member lists
- Event logging for key operations

### 8.4 Storage Optimization

```rust
enum DataKey {
    Admin,
    Config,
    CurrentRound,
    InsurancePool,
    Member(Address),
    Round(u64),
    Statistics,
}
```

---

## 9. Security Considerations

### 9.1 Reentrancy Attacks

- Use atomicity guarantees from the Soroban SDK
- State updates occur before transfers

### 9.2 Integer Overflow

- Use checked arithmetic
- Amount range validation

### 9.3 Authorization Checks

```rust
member.require_auth();
```

### 9.4 Denial of Service

- Limit maximum member count (e.g., 100 members)
- Prevent malicious mass joining

### 9.5 Front-running

- Prevent payout races (only one person may receive per round)
- First-come-first-served, or use a queuing mechanism

---

## 10. Deployment and Upgrades

### 10.1 Deployment Flow

```bash
# Build the contract
cargo build --target wasm32-unknown-unknown --release

# Optimize WASM
soroban contract optimize --wasm target/wasm32-unknown-unknown/release/rosca.wasm

# Deploy
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/rosca.wasm \
  --source ADMIN_SECRET_KEY \
  --network testnet

# Initialize
soroban contract invoke \
  --id CONTRACT_ID \
  --source ADMIN_SECRET_KEY \
  --network testnet \
  -- initialize \
  --admin ADMIN_ADDRESS \
  --contribution-amount 1000000000 \
  --contribution-period 604800 \
  ...
```

### 10.2 Upgrade Strategy

If the contract needs to be upgraded:
- Use the upgradeable contract pattern
- Or deploy a new version and migrate data

---

## 11. Testing Plan

### 11.1 Unit Tests

- Join/exit logic
- Contribution/payout logic
- Violation handling logic
- Debt calculation
- Deposit management

### 11.2 Integration Tests

- Complete round flow
- Multi-user concurrent operations
- Edge cases

### 11.3 Scenario Tests

- 10 members running normally for 20 rounds
- Violation scenarios (single and multiple violators)
- Dynamic membership changes
- Insurance pool depletion and recovery

---

## 12. Frontend Integration Recommendations

### 12.1 User Interface

- Current round information
- Personal debt status (net_balance)
- Payout countdown (cooldown period)
- Contribution reminders
- Insurance pool balance (transparency)

### 12.2 Notifications

- Contribution deadline reminders
- Violation warnings
- Payout opportunity notifications
- Low deposit warnings

### 12.3 Data Display

- Historical round records
- Violation statistics
- Member list
- Personal contribution/payout history

---

## 13. Summary

### 13.1 Core Advantages

1. **Fair and Transparent**: Dual tracking system (amount + points); everyone's contributions and receipts are balanced
2. **Controlled Risk**: Three-layer guarantee mechanism reduces the impact of defaults
3. **Flexible Participation**: Dynamic joining/exiting with no fixed end time
4. **Incentive-Compatible**: Violators bear the consequences (deposit + points + lockout); compliant members are protected
5. **Automated Fairness**: Automatic ranking based on points; priority is transparent and predictable

### 13.2 Innovations

1. **Dual Tracking System**:
   - Debt system (amount dimension): ensures monetary balance
   - Points system (count dimension): determines payout priority
2. **Dynamic Membership**: Breaks the traditional ROSCA fixed-membership constraint
3. **Insurance Pool**: Introduces a risk-sharing mechanism to buffer default shocks
4. **Observation Period**: Guards against new-member misconduct
5. **Multiple Violation Penalties**: Deposit deduction + points reduction + time lockout

### 13.3 Items for Further Discussion

1. Choice of cooldown period type (fixed vs. dynamic vs. time-based)
2. Handling of funds when no one receives (refund vs. retain vs. accumulate)
3. Minimum member count restriction
4. Whether a voting/governance mechanism is needed

---

## Appendix: Recommended Parameter Configurations

### Small-Scale Mutual Aid (1,000 USDC)

```rust
RoscaConfig {
    contribution_amount: 1_000_000_000,  // 1,000 USDC (7 decimal places)
    contribution_period: 604_800,        // 7 days

    min_deposit: 3_000_000_000,
    recommended_deposit: 5_000_000_000,
    max_deposit: Some(10_000_000_000),

    insurance_rate: 2,                   // 2%
    max_insurance_pool: 20_000_000_000,
    max_insurance_coverage: 2_000_000_000,

    observation_contributions: 3,
    all_members_observation: true,

    cooldown_type: DynamicMembers,

    // Violation configuration (progressive)
    violation_grace_period: 86_400,      // 24 hours
    violation_penalties: vec![
        ViolationPenalty {
            deposit_deduction: 1_000_000_000,  // 1,000
            points_deduction: 3,
            lockout_rounds: 2,
        },
        ViolationPenalty {
            deposit_deduction: 2_000_000_000,  // 2,000
            points_deduction: 5,
            lockout_rounds: 5,
        },
    ],
    max_violations: 3,

    // Late fee configuration (progressive)
    late_fee_rates: vec![5, 10, 20],     // 5%, 10%, 20%
    max_late_count: 3,

    // Beneficiary protection
    max_beneficiary_loss_rate: 10,       // 10%

    // Administration
    admin: ADMIN_ADDRESS,
    allow_join: true,
}
```

### Large-Scale Mutual Aid (10,000 USDC)

```rust
RoscaConfig {
    contribution_amount: 10_000_000_000,
    contribution_period: 2_592_000,      // 30 days

    min_deposit: 30_000_000_000,
    recommended_deposit: 50_000_000_000,
    max_deposit: Some(100_000_000_000),

    insurance_rate: 3,                   // 3% (higher risk)
    max_insurance_pool: 200_000_000_000,
    max_insurance_coverage: 30_000_000_000,

    observation_contributions: 5,        // longer observation period
    all_members_observation: true,

    cooldown_type: TimeBased(15_552_000), // 180 days

    // Violation configuration (progressive, stricter)
    violation_grace_period: 259_200,     // 3 days
    violation_penalties: vec![
        ViolationPenalty {
            deposit_deduction: 15_000_000_000,  // 15,000 (1.5x)
            points_deduction: 5,
            lockout_rounds: 3,
        },
        ViolationPenalty {
            deposit_deduction: 30_000_000_000,  // 30,000 (3x)
            points_deduction: 10,
            lockout_rounds: 7,
        },
    ],
    max_violations: 2,                   // stricter (kicked out after 2 violations)

    // Late fee configuration (progressive)
    late_fee_rates: vec![10, 20, 30],    // 10%, 20%, 30%
    max_late_count: 3,

    // Beneficiary protection
    max_beneficiary_loss_rate: 5,        // 5% (stricter)

    // Administration
    admin: ADMIN_ADDRESS,
    allow_join: true,
}
```

---

**Document Version:** v2.1
**Last Updated:** 2025-01-08
**Status:** Pending Review

**v2.1 Update Notes (Core Decision Integration):**
- ✅ **Insurance pool mechanism**: 2% rate + surplus refund mechanism
- ✅ **Violation penalties**: Changed to progressive (1st time -3 points, 2nd time -5 points, 3rd time kicked out)
- ✅ **Late fees**: Changed to progressive (5% → 10% → 20%)
- ✅ **Beneficiary protection**: Set maximum loss rate at 10%
- ✅ **No-recipient handling**: Refund contributions to this round's contributors
- ✅ **Cooldown period**: Retained dynamic cooldown period mechanism
- ✅ Added `late_count` field to track late contribution count
- ✅ Added `max_beneficiary_loss_rate` protection parameter
- ✅ Updated all configuration parameter examples

**v2.0 Update Notes:**
- ✅ Added detailed points system design (1 point = 1 contribution)
- ✅ Added automatic payout priority ranking mechanism
- ✅ Added the impact of violations on points and the lockout period mechanism
- ✅ Updated member data structure with points-related fields
- ✅ Added Example 9: Points priority ranking demonstration
- ✅ Updated all examples with points calculation steps
- ✅ Improved frontend display recommendations (avoid showing negative scores directly)
