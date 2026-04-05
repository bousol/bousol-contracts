use soroban_sdk::{contracttype, Address};

/// Instance storage keys — data that lives as long as the contract instance
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum DataKey {
    // Configuration
    Config,

    // State
    CurrentRound,
    StartTime,
    InsurancePool,

    // Members
    Member(Address),
    MembersList,  // Vec<Address>

    // Statistics
    Statistics,

    // Admin
    Admin,                     // Address - admin who can call settle_round

    // Governance
    ProposalCounter,           // u64 - auto-increment counter

    // Per-round insurance tracking (reset each round)
    RoundInsurance,            // i128 - actual insurance collected in current round

    // Pause tracking
    PauseTime,                 // u64 - timestamp when ROSCA was paused
}

/// Persistent storage keys — historical data with individual TTL
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum PersistentDataKey {
    Round(u64),                // Round data by round number
    Proposal(u64),             // Proposal by ID
    Vote(u64, Address),        // Vote by proposal ID and voter address
}

/// Temporary storage keys — auto-expiring data
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum TempDataKey {
    Sponsor(Address),          // Sponsor record for a candidate address
}
