use soroban_sdk::{contracttype, Address};

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

    // Rounds
    Round(u64),

    // Statistics
    Statistics,

    // Sponsorship
    Sponsor(Address),          // Sponsor record for a candidate address

    // Governance
    ProposalCounter,           // u64 - auto-increment counter
    Proposal(u64),             // Proposal by ID
    Vote(u64, Address),        // Vote by proposal ID and voter address
}
