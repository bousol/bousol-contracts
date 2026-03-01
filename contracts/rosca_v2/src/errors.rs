use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
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
