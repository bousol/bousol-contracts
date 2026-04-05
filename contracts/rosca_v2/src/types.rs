use soroban_sdk::{contracttype, Address, Env, Vec};

/// ROSCA status
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum RoscaStatus {
    Active,      // Normal operation
    Paused,      // Paused (can be resumed via Resume proposal)
    Dissolved,   // Dissolved (cannot be resumed)
}

/// Member status
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum MemberStatus {
    Observing,   // In observation period
    Active,      // Active member
    ExitPending, // Exit requested
    Kicked,      // Kicked out
}

/// Cooldown period type
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum CooldownType {
    FixedRounds(u64),   // Fixed number of rounds
    DynamicMembers,     // Dynamic = member count at time of receipt
    TimeBased(u64),     // Time-based (in seconds)
}

/// Violation penalty configuration
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct ViolationPenalty {
    pub deposit_deduction: i128,    // Amount to deduct from deposit
    pub points_deduction: u32,      // Points to deduct
    pub lockout_rounds: u64,        // Number of rounds to lock out
}

/// ROSCA configuration
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct RoscaConfig {
    // Basic parameters
    pub contribution_amount: i128,      // Contribution amount per round
    pub contribution_period: u64,       // Contribution period in seconds

    // Deposit configuration
    pub min_deposit: i128,              // Minimum deposit required
    pub recommended_deposit: i128,      // Recommended deposit amount
    pub max_deposit: Option<i128>,      // Maximum deposit allowed

    // Insurance mechanism
    pub insurance_rate: u32,            // Insurance fee rate (e.g., 2 = 2%)
    pub max_insurance_pool: i128,       // Maximum insurance pool size
    pub max_insurance_coverage: i128,   // Maximum insurance coverage per round

    // Observation period
    pub observation_contributions: u32, // Required contributions during observation
    pub all_members_observation: bool,  // Whether all members need observation period

    // Cooldown period
    pub cooldown_type: CooldownType,

    // Violation configuration (progressive)
    pub violation_grace_period: u64,    // Grace period in seconds
    pub violation_penalties: Vec<ViolationPenalty>, // Progressive penalty configuration
    pub max_violations: u32,            // Maximum violations before kick

    // Late fee configuration (progressive)
    pub late_fee_rates: Vec<u32>,       // Late fee rates (e.g., [5, 10, 20] for %)
    pub max_late_count: u32,            // Maximum late count allowed

    // Beneficiary protection
    pub max_beneficiary_loss_rate: u32, // Maximum loss rate (e.g., 10 = 10%)

    // Capacity
    pub max_members: u32,               // Maximum number of members allowed

    // Administration
    pub allow_join: bool,               // Whether new members can join
    pub require_sponsor: bool,          // Whether new members need a sponsor
    pub status: RoscaStatus,            // ROSCA status

    // Token
    pub token_address: Address,         // Token contract address for contributions
}

/// Member data
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct Member {
    // Basic information
    pub address: Address,
    pub joined_at: u64,                 // Timestamp when member joined
    pub status: MemberStatus,
    pub is_system_account: bool,        // Backend service account (doesn't receive payout)

    // === Debt system (amount dimension) ===
    pub deposit: i128,                  // Current deposit balance
    pub total_contributed: i128,        // Total amount contributed
    pub total_received: i128,           // Total amount received

    // === Points system (count dimension) ===
    pub contribution_count: u32,        // Number of contributions made
    pub receive_count: u32,             // Number of times received payout
    pub violation_count: u32,           // Number of violations
    pub late_count: u32,                // Number of late contributions
    pub on_time_streak: u32,           // Consecutive on-time contributions

    // === Statistics ===
    pub observation_count: u32,         // Contributions made during observation
    pub last_contribution_round: u64,   // Last round contributed
    pub last_received_round: u64,       // Last round received payout

    // === State control ===
    pub cooldown_until_round: u64,      // Cooldown ends at this round
    pub violation_lockout_until: u64,   // Violation lockout ends at this round

    // === Sponsorship ===
    pub sponsored_by: Option<Address>,  // Who sponsored this member (audit trail)
}

impl Member {
    /// Net balance (debt system - amount)
    pub fn net_balance(&self) -> i128 {
        self.total_contributed - self.total_received
    }

    /// Priority score (points system - count)
    /// Uses i64 internally to prevent overflow with large member counts
    pub fn priority_score(&self, members_count: u32, config: &RoscaConfig) -> i64 {
        let violation_penalty = if self.violation_count == 0 {
            0i64
        } else {
            // Calculate cumulative violation penalty
            let mut total_penalty = 0i64;
            for i in 0..self.violation_count {
                if let Some(penalty) = config.violation_penalties.get(i as u32) {
                    total_penalty += penalty.points_deduction as i64;
                } else {
                    // Out of range, use last configured penalty
                    if let Some(last) = config.violation_penalties.last() {
                        total_penalty += last.points_deduction as i64;
                    }
                }
            }
            total_penalty
        };

        self.contribution_count as i64
            - (self.receive_count as i64 * members_count as i64)
            - violation_penalty
    }

    /// Voting weight (for governance)
    pub fn voting_weight(&self) -> u32 {
        // Voting weight based on contribution count
        self.contribution_count
    }

    /// Calculate late fee with discount based on on-time streak
    pub fn calculate_late_fee(&self, base_fee: i128) -> i128 {
        // Consecutive 20+ on-time: 80% discount
        // Consecutive 10-19 on-time: 50% discount
        // Less than 10: no discount
        if self.on_time_streak >= 20 {
            base_fee * 20 / 100  // Pay only 20%
        } else if self.on_time_streak >= 10 {
            base_fee * 50 / 100  // Pay only 50%
        } else {
            base_fee  // Pay full amount
        }
    }

    /// Check if member can receive payout (multiple conditions)
    pub fn can_receive(&self, _env: &Env, current_round: u64, members_count: u32, config: &RoscaConfig) -> bool {
        !self.is_system_account  // System accounts are excluded from payout
            && self.status == MemberStatus::Active
            && self.net_balance() >= 0
            && self.priority_score(members_count, config) > 0i64
            && self.observation_count >= config.observation_contributions
            && current_round >= self.cooldown_until_round
            && current_round >= self.violation_lockout_until
    }

    /// Check if member can exit
    pub fn can_exit(&self) -> bool {
        self.net_balance() >= 0
    }
}

/// Round data
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct Round {
    pub round_id: u64,
    pub start_time: u64,
    pub end_time: u64,

    // Participants
    pub expected_contributors: Vec<Address>,  // Members expected to contribute
    pub actual_contributors: Vec<Address>,    // Members who actually contributed
    pub violators: Vec<Address>,              // Members who violated

    // Funds
    pub total_collected: i128,                // Total amount collected
    pub insurance_collected: i128,            // Insurance fee collected
    pub recipient: Option<Address>,           // Payout recipient
    pub payout_amount: i128,                  // Actual payout amount

    // Actual insurance deducted from contributions this round
    pub actual_insurance: i128,               // Actual insurance added to pool (may be less than theoretical if pool was full)

    // Compensation
    pub violations_loss: i128,                // Loss from violations
    pub deposit_compensation: i128,           // Compensation from deposits
    pub insurance_compensation: i128,         // Compensation from insurance pool
    pub beneficiary_loss: i128,              // Loss borne by beneficiary
}

/// Statistics
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct Statistics {
    pub total_rounds: u64,
    pub total_members: u32,
    pub active_members: u32,
    pub total_contributed: i128,
    pub total_paid_out: i128,
    pub insurance_pool: i128,
    pub total_violations: u32,
}

/// Dissolution mode
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum DissolutionMode {
    Emergency,  // Emergency dissolution (>75%, 24h)
    Normal,     // Normal dissolution (>90%, 14d)
}

/// Emergency payout details
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct EmergencyPayoutDetails {
    pub requester: Address,
    pub amount: i128,
}

/// Proposal type
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum ProposalType {
    EmergencyPayout(EmergencyPayoutDetails),  // Emergency payout (>66%, 48h)
    UpdateConfig(RoscaConfig),                // Config changes (>50%, 7d + 7d cooldown)
    Dissolution(DissolutionMode),             // Dissolution: Emergency (>75%, 24h) or Normal (>90%, 14d)
    Pause,                                    // Pause ROSCA (>66%, 48h)
    Resume,                                   // Resume ROSCA (>50%, 48h)
}

/// Proposal
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub proposal_type: ProposalType,
    pub created_at: u64,
    pub voting_ends_at: u64,
    pub votes_for_weight: u32,      // Total voting weight for
    pub votes_against_weight: u32,  // Total voting weight against
    pub executed: bool,
    pub cooldown_ends_at: Option<u64>,  // For config updates (7d cooldown after voting)
}

/// Vote choice
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum VoteChoice {
    For,
    Against,
}

/// Vote record
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct Vote {
    pub voter: Address,
    pub proposal_id: u64,
    pub choice: VoteChoice,
    pub weight: u32,  // Voting weight at time of vote
    pub voted_at: u64,
}
