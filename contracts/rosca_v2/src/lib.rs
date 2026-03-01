#![no_std]

mod errors;
mod storage;
mod types;

use errors::Error;
use storage::DataKey;
use types::*;

use soroban_sdk::{contract, contractimpl, token, Address, Env, Vec};

#[contract]
pub struct RoscaV2Contract;

#[contractimpl]
impl RoscaV2Contract {
    /// Initialize the contract
    pub fn initialize(env: Env, config: RoscaConfig) -> Result<(), Error> {
        // Check if already initialized
        if env.storage().instance().has(&DataKey::Config) {
            return Err(Error::AlreadyInitialized);
        }

        // Validate configuration
        Self::validate_config(&config)?;

        // Store configuration
        env.storage().instance().set(&DataKey::Config, &config);

        // Initialize state
        env.storage().instance().set(&DataKey::CurrentRound, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::StartTime, &env.ledger().timestamp());
        env.storage().instance().set(&DataKey::InsurancePool, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::MembersList, &Vec::<Address>::new(&env));

        // Initialize statistics
        let stats = Statistics {
            total_rounds: 0,
            total_members: 0,
            active_members: 0,
            total_contributed: 0,
            total_paid_out: 0,
            insurance_pool: 0,
            total_violations: 0,
        };
        env.storage().instance().set(&DataKey::Statistics, &stats);

        Ok(())
    }

    /// Get configuration
    pub fn get_config(env: Env) -> Result<RoscaConfig, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(Error::NotInitialized)
    }

    /// Get current round number
    pub fn get_current_round(env: Env) -> Result<u64, Error> {
        env.storage()
            .instance()
            .get(&DataKey::CurrentRound)
            .ok_or(Error::NotInitialized)
    }

    /// Get insurance pool balance
    pub fn get_insurance_pool(env: Env) -> Result<i128, Error> {
        env.storage()
            .instance()
            .get(&DataKey::InsurancePool)
            .ok_or(Error::NotInitialized)
    }

    /// Get member information
    pub fn get_member(env: Env, address: Address) -> Result<Member, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Member(address))
            .ok_or(Error::MemberNotFound)
    }

    /// Get list of all member addresses
    pub fn get_members(env: Env) -> Result<Vec<Address>, Error> {
        env.storage()
            .instance()
            .get(&DataKey::MembersList)
            .ok_or(Error::NotInitialized)
    }

    /// Get statistics
    pub fn get_statistics(env: Env) -> Result<Statistics, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Statistics)
            .ok_or(Error::NotInitialized)
    }

    /// Get round information
    pub fn get_round(env: Env, round_id: u64) -> Result<Round, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Round(round_id))
            .ok_or(Error::InvalidState)
    }

    /// Dissolve ROSCA and refund all members
    /// This function is now only callable via voting (execute_proposal)
    fn dissolve_internal(env: Env) -> Result<(), Error> {
        let mut config = Self::get_config(env.clone())?;

        if config.status == RoscaStatus::Dissolved {
            return Err(Error::RoscaDissolved);
        }

        let members = Self::get_members(env.clone())?;
        let token_client = token::Client::new(&env, &config.token_address);

        // Refund all members: deposit + positive net balance
        for member_addr in members.iter() {
            let member = Self::get_member(env.clone(), member_addr.clone())?;

            let mut refund = member.deposit;
            let net_balance = member.net_balance();
            if net_balance > 0 {
                refund += net_balance;
            }

            if refund > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &member_addr,
                    &refund
                );
            }
        }

        // Distribute insurance pool to all active members
        let insurance_pool = Self::get_insurance_pool(env.clone())?;
        let active_count = Self::get_active_members_count(env.clone())?;
        if insurance_pool > 0 && active_count > 0 {
            let share = insurance_pool / active_count as i128;
            for member_addr in members.iter() {
                let member = Self::get_member(env.clone(), member_addr.clone())?;
                if member.status == MemberStatus::Active {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &member_addr,
                        &share
                    );
                }
            }
        }

        // Mark as dissolved
        config.status = RoscaStatus::Dissolved;
        env.storage().instance().set(&DataKey::Config, &config);

        Ok(())
    }

    /// Create a new proposal
    pub fn propose(env: Env, proposer: Address, proposal_type: ProposalType) -> Result<u64, Error> {
        proposer.require_auth();

        let config = Self::get_config(env.clone())?;

        // Check ROSCA status
        if config.status != RoscaStatus::Active {
            return Err(Error::RoscaNotActive);
        }

        // Check proposer is an active member
        let proposer_member = Self::get_member(env.clone(), proposer.clone())?;
        if proposer_member.status != MemberStatus::Active {
            return Err(Error::MemberNotActive);
        }

        // Get or initialize proposal counter
        let proposal_id: u64 = env.storage()
            .instance()
            .get(&DataKey::ProposalCounter)
            .unwrap_or(0);

        // Determine voting period and thresholds based on proposal type
        let current_time = env.ledger().timestamp();
        let (voting_duration, cooldown_duration) = match &proposal_type {
            ProposalType::EmergencyPayout(_) => (48 * 3600, None),  // 48 hours
            ProposalType::UpdateConfig => (7 * 24 * 3600, Some(7 * 24 * 3600)),  // 7d voting + 7d cooldown
            ProposalType::Dissolution(mode) => match mode {
                DissolutionMode::Emergency => (24 * 3600, None),  // 24 hours
                DissolutionMode::Normal => (14 * 24 * 3600, None), // 14 days
            },
        };

        let proposal = Proposal {
            id: proposal_id,
            proposer: proposer.clone(),
            proposal_type: proposal_type.clone(),
            created_at: current_time,
            voting_ends_at: current_time + voting_duration,
            votes_for_weight: 0,
            votes_against_weight: 0,
            executed: false,
            cooldown_ends_at: cooldown_duration.map(|d| current_time + voting_duration + d),
        };

        // Store proposal
        env.storage()
            .instance()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        // Increment proposal counter
        env.storage()
            .instance()
            .set(&DataKey::ProposalCounter, &(proposal_id + 1));

        Ok(proposal_id)
    }

    /// Vote on a proposal
    pub fn vote(env: Env, voter: Address, proposal_id: u64, choice: VoteChoice) -> Result<(), Error> {
        voter.require_auth();

        let config = Self::get_config(env.clone())?;

        // Check ROSCA status
        if config.status != RoscaStatus::Active {
            return Err(Error::RoscaNotActive);
        }

        // Check voter is an active member
        let voter_member = Self::get_member(env.clone(), voter.clone())?;
        if voter_member.status != MemberStatus::Active {
            return Err(Error::MemberNotActive);
        }

        // Get proposal
        let mut proposal = env.storage()
            .instance()
            .get::<DataKey, Proposal>(&DataKey::Proposal(proposal_id))
            .ok_or(Error::ProposalNotFound)?;

        // Check if proposal is already executed
        if proposal.executed {
            return Err(Error::ProposalAlreadyExecuted);
        }

        // Check if voting period has ended
        let current_time = env.ledger().timestamp();
        if current_time > proposal.voting_ends_at {
            return Err(Error::VotingPeriodEnded);
        }

        // Check if already voted
        if env.storage().instance().has(&DataKey::Vote(proposal_id, voter.clone())) {
            return Err(Error::AlreadyVoted);
        }

        // Calculate voting weight
        let voting_weight = voter_member.voting_weight();

        // Create vote record
        let vote = Vote {
            voter: voter.clone(),
            proposal_id,
            choice: choice.clone(),
            weight: voting_weight,
            voted_at: current_time,
        };

        // Store vote
        env.storage()
            .instance()
            .set(&DataKey::Vote(proposal_id, voter.clone()), &vote);

        // Update proposal vote counts
        match choice {
            VoteChoice::For => proposal.votes_for_weight += voting_weight,
            VoteChoice::Against => proposal.votes_against_weight += voting_weight,
        }

        env.storage()
            .instance()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        Ok(())
    }

    /// Execute a proposal after voting ends
    pub fn execute_proposal(env: Env, executor: Address, proposal_id: u64) -> Result<(), Error> {
        executor.require_auth();

        let config = Self::get_config(env.clone())?;

        // Check ROSCA status (allow execution even if paused for dissolution)
        if config.status == RoscaStatus::Dissolved {
            return Err(Error::RoscaDissolved);
        }

        // Check executor is a member
        let executor_member = Self::get_member(env.clone(), executor.clone())?;
        if executor_member.status != MemberStatus::Active {
            return Err(Error::MemberNotActive);
        }

        // Get proposal
        let mut proposal = env.storage()
            .instance()
            .get::<DataKey, Proposal>(&DataKey::Proposal(proposal_id))
            .ok_or(Error::ProposalNotFound)?;

        // Check if already executed
        if proposal.executed {
            return Err(Error::ProposalAlreadyExecuted);
        }

        // Check if voting period has ended
        let current_time = env.ledger().timestamp();
        if current_time < proposal.voting_ends_at {
            return Err(Error::VotingPeriodNotEnded);
        }

        // Check cooldown for UpdateConfig proposals
        if let Some(cooldown_ends) = proposal.cooldown_ends_at {
            if current_time < cooldown_ends {
                return Err(Error::CooldownNotEnded);
            }
        }

        // Calculate total voting weight
        let members = Self::get_members(env.clone())?;
        let mut total_voting_weight = 0u32;
        for member_addr in members.iter() {
            let member = Self::get_member(env.clone(), member_addr)?;
            if member.status == MemberStatus::Active {
                total_voting_weight += member.voting_weight();
            }
        }

        // Check if proposal passes based on type
        let passes = match &proposal.proposal_type {
            ProposalType::EmergencyPayout(_) => {
                // >66% approval required
                proposal.votes_for_weight * 100 > total_voting_weight * 66
            },
            ProposalType::UpdateConfig => {
                // >50% approval required
                proposal.votes_for_weight * 100 > total_voting_weight * 50
            },
            ProposalType::Dissolution(mode) => {
                match mode {
                    DissolutionMode::Emergency => {
                        // >75% approval required
                        proposal.votes_for_weight * 100 > total_voting_weight * 75
                    },
                    DissolutionMode::Normal => {
                        // >90% approval required
                        proposal.votes_for_weight * 100 > total_voting_weight * 90
                    },
                }
            },
        };

        if !passes {
            return Err(Error::InsufficientVotes);
        }

        // Execute proposal based on type
        match &proposal.proposal_type {
            ProposalType::EmergencyPayout(details) => {
                // Verify requester is a member
                let mut requester_member = Self::get_member(env.clone(), details.requester.clone())?;

                // Check if requester has sufficient net balance
                let net_balance = requester_member.net_balance();
                if net_balance < details.amount {
                    return Err(Error::InsufficientNetBalance);
                }

                // Transfer emergency payout
                let token_client = token::Client::new(&env, &config.token_address);
                token_client.transfer(
                    &env.current_contract_address(),
                    &details.requester,
                    &details.amount
                );

                // Update member data
                requester_member.total_received += details.amount;
                requester_member.receive_count += 1;
                requester_member.last_received_round = Self::get_current_round(env.clone())?;

                // Set cooldown period (same as normal payout)
                let members_count = Self::get_active_members_count(env.clone())?;
                let current_round = Self::get_current_round(env.clone())?;
                requester_member.cooldown_until_round = match config.cooldown_type {
                    CooldownType::FixedRounds(rounds) => current_round + rounds,
                    CooldownType::DynamicMembers => current_round + members_count as u64,
                    CooldownType::TimeBased(_) => current_round + members_count as u64,
                };

                env.storage()
                    .instance()
                    .set(&DataKey::Member(details.requester.clone()), &requester_member);

                // Update statistics
                let mut stats = Self::get_statistics(env.clone())?;
                stats.total_paid_out += details.amount;
                env.storage().instance().set(&DataKey::Statistics, &stats);
            },
            ProposalType::UpdateConfig => {
                // UpdateConfig logic would go here
                // For now, just mark as executed
            },
            ProposalType::Dissolution(_) => {
                // Execute dissolution
                Self::dissolve_internal(env.clone())?;
            },
        }

        // Mark proposal as executed
        proposal.executed = true;
        env.storage()
            .instance()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        Ok(())
    }

    /// Get proposal information
    pub fn get_proposal(env: Env, proposal_id: u64) -> Result<Proposal, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(Error::ProposalNotFound)
    }

    /// Get vote record
    pub fn get_vote(env: Env, proposal_id: u64, voter: Address) -> Result<Vote, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Vote(proposal_id, voter))
            .ok_or(Error::InvalidState)
    }

    /// Member join
    pub fn join(env: Env, member: Address, deposit_amount: i128) -> Result<(), Error> {
        member.require_auth();

        let config: RoscaConfig = Self::get_config(env.clone())?;

        // Check ROSCA status
        if config.status != RoscaStatus::Active {
            return Err(Error::RoscaNotActive);
        }

        // Check if joining is allowed
        if !config.allow_join {
            return Err(Error::JoinNotAllowed);
        }

        // Check if member already exists
        if env.storage().instance().has(&DataKey::Member(member.clone())) {
            return Err(Error::MemberAlreadyExists);
        }

        // Check deposit amount
        if deposit_amount < config.min_deposit {
            return Err(Error::InsufficientDeposit);
        }
        if let Some(max_deposit) = config.max_deposit {
            if deposit_amount > max_deposit {
                return Err(Error::InsufficientDeposit);
            }
        }

        // Create new member
        let new_member = Member {
            address: member.clone(),
            joined_at: env.ledger().timestamp(),
            status: if config.all_members_observation {
                MemberStatus::Observing
            } else {
                MemberStatus::Active
            },
            is_system_account: false,
            deposit: deposit_amount,
            total_contributed: 0,
            total_received: 0,
            contribution_count: 0,
            receive_count: 0,
            violation_count: 0,
            late_count: 0,
            on_time_streak: 0,
            observation_count: 0,
            last_contribution_round: u64::MAX, // Never contributed yet
            last_received_round: u64::MAX,     // Never received yet
            cooldown_until_round: 0,
            violation_lockout_until: 0,
        };

        // Store member
        env.storage()
            .instance()
            .set(&DataKey::Member(member.clone()), &new_member);

        // Add to members list
        let mut members: Vec<Address> = Self::get_members(env.clone())?;
        members.push_back(member.clone());
        env.storage().instance().set(&DataKey::MembersList, &members);

        // Update statistics
        let mut stats = Self::get_statistics(env.clone())?;
        stats.total_members += 1;
        stats.active_members += 1;
        env.storage().instance().set(&DataKey::Statistics, &stats);

        // Transfer deposit from member to contract
        let token_client = token::Client::new(&env, &config.token_address);
        token_client.transfer(&member, &env.current_contract_address(), &deposit_amount);

        Ok(())
    }

    /// Member exit
    pub fn exit(env: Env, member: Address) -> Result<(), Error> {
        member.require_auth();

        let config = Self::get_config(env.clone())?;
        let member_data = Self::get_member(env.clone(), member.clone())?;

        // Check if member can exit
        if !member_data.can_exit() {
            return Err(Error::CannotExit);
        }

        // Calculate refund amount (deposit + positive net balance)
        let mut refund_amount = member_data.deposit;
        let net_balance = member_data.net_balance();
        if net_balance > 0 {
            refund_amount += net_balance;
        }

        // Remove from members list
        let members: Vec<Address> = Self::get_members(env.clone())?;
        let mut new_members = Vec::new(&env);
        for addr in members.iter() {
            if addr != member {
                new_members.push_back(addr);
            }
        }
        env.storage().instance().set(&DataKey::MembersList, &new_members);

        // Delete member data
        env.storage().instance().remove(&DataKey::Member(member.clone()));

        // Update statistics
        let mut stats = Self::get_statistics(env.clone())?;
        stats.active_members = stats.active_members.saturating_sub(1);
        env.storage().instance().set(&DataKey::Statistics, &stats);

        // Transfer refund to member
        if refund_amount > 0 {
            let token_client = token::Client::new(&env, &config.token_address);
            token_client.transfer(&env.current_contract_address(), &member, &refund_amount);
        }

        Ok(())
    }

    /// Contribution
    pub fn contribute(env: Env, member: Address) -> Result<(), Error> {
        member.require_auth();

        let config = Self::get_config(env.clone())?;

        // Check ROSCA status
        if config.status != RoscaStatus::Active {
            return Err(Error::RoscaNotActive);
        }

        let current_round = Self::get_current_round(env.clone())?;
        let mut member_data = Self::get_member(env.clone(), member.clone())?;

        // Check member status
        if member_data.status != MemberStatus::Active && member_data.status != MemberStatus::Observing {
            return Err(Error::MemberNotActive);
        }

        // Check if already contributed
        if member_data.last_contribution_round == current_round {
            return Err(Error::AlreadyContributed);
        }

        // Check contribution period
        let start_time: u64 = env.storage()
            .instance()
            .get(&DataKey::StartTime)
            .ok_or(Error::NotInitialized)?;
        let current_time = env.ledger().timestamp();
        let period_start = start_time + (current_round * config.contribution_period);
        let period_end = period_start + config.contribution_period;

        if current_time < period_start {
            return Err(Error::ContributionPeriodNotStarted);
        }
        if current_time > period_end {
            return Err(Error::ContributionPeriodEnded);
        }

        // Calculate amount distribution
        let insurance_amount = (config.contribution_amount as i128 * config.insurance_rate as i128) / 100;
        let _pool_amount = config.contribution_amount - insurance_amount;

        // Update member data
        member_data.total_contributed += config.contribution_amount;
        member_data.contribution_count += 1;
        member_data.last_contribution_round = current_round;

        // Increment on-time streak for on-time contributions
        member_data.on_time_streak += 1;

        // Update observation count
        if member_data.status == MemberStatus::Observing {
            member_data.observation_count += 1;
            if member_data.observation_count >= config.observation_contributions {
                member_data.status = MemberStatus::Active;
            }
        }

        env.storage()
            .instance()
            .set(&DataKey::Member(member.clone()), &member_data);

        // Update insurance pool
        let mut insurance_pool = Self::get_insurance_pool(env.clone())?;
        insurance_pool += insurance_amount;
        env.storage()
            .instance()
            .set(&DataKey::InsurancePool, &insurance_pool);

        // Update statistics
        let mut stats = Self::get_statistics(env.clone())?;
        stats.total_contributed += config.contribution_amount;
        stats.insurance_pool = insurance_pool;
        env.storage().instance().set(&DataKey::Statistics, &stats);

        // Transfer contribution amount from member to contract
        let token_client = token::Client::new(&env, &config.token_address);
        token_client.transfer(&member, &env.current_contract_address(), &config.contribution_amount);

        Ok(())
    }

    /// Late contribution (with late fee)
    pub fn contribute_late(env: Env, member: Address) -> Result<(), Error> {
        member.require_auth();

        let config = Self::get_config(env.clone())?;

        // Check ROSCA status
        if config.status != RoscaStatus::Active {
            return Err(Error::RoscaNotActive);
        }

        let mut member_data = Self::get_member(env.clone(), member.clone())?;

        // Check late count
        if member_data.late_count >= config.max_late_count {
            return Err(Error::MaxLateCountReached);
        }

        // Check grace period
        let current_round = Self::get_current_round(env.clone())?;
        let start_time: u64 = env.storage()
            .instance()
            .get(&DataKey::StartTime)
            .ok_or(Error::NotInitialized)?;
        let current_time = env.ledger().timestamp();
        let period_start = start_time + (current_round * config.contribution_period);
        let period_end = period_start + config.contribution_period;
        let grace_period_end = period_end + config.violation_grace_period;

        if current_time <= period_end {
            return Err(Error::ContributionPeriodNotStarted); // Should use normal contribute
        }
        if current_time > grace_period_end {
            return Err(Error::GracePeriodEnded);
        }

        // Calculate late fee (progressive based on late_count, with on_time_streak discount)
        let late_fee_rate = config.late_fee_rates.get(member_data.late_count).unwrap_or(20);
        let base_late_fee = (config.contribution_amount * late_fee_rate as i128) / 100;
        let late_fee = member_data.calculate_late_fee(base_late_fee);
        let total_amount = config.contribution_amount + late_fee;

        // Execute normal contribution logic (duplicated to avoid time check conflict)
        let insurance_amount = (config.contribution_amount as i128 * config.insurance_rate as i128) / 100;

        // Update member data
        member_data.total_contributed += config.contribution_amount;
        member_data.contribution_count += 1;
        member_data.last_contribution_round = current_round;
        member_data.late_count += 1;

        // Reset on-time streak for late contributions
        member_data.on_time_streak = 0;

        // Update observation count
        if member_data.status == MemberStatus::Observing {
            member_data.observation_count += 1;
            if member_data.observation_count >= config.observation_contributions {
                member_data.status = MemberStatus::Active;
            }
        }

        env.storage()
            .instance()
            .set(&DataKey::Member(member.clone()), &member_data);

        // Update insurance pool (normal insurance + late fee)
        let mut insurance_pool = Self::get_insurance_pool(env.clone())?;
        insurance_pool += insurance_amount + late_fee;
        env.storage()
            .instance()
            .set(&DataKey::InsurancePool, &insurance_pool);

        // Update statistics
        let mut stats = Self::get_statistics(env.clone())?;
        stats.total_contributed += config.contribution_amount;
        stats.insurance_pool = insurance_pool;
        env.storage().instance().set(&DataKey::Statistics, &stats);

        // Transfer total amount (contribution + late fee) from member to contract
        let token_client = token::Client::new(&env, &config.token_address);
        token_client.transfer(&member, &env.current_contract_address(), &total_amount);

        Ok(())
    }

    /// Calculate recipient for current round (priority-based, deterministic query)
    /// This function is for querying who has highest priority, non-consuming, for reference only
    pub fn calculate_recipient(env: Env) -> Result<Option<Address>, Error> {
        let config = Self::get_config(env.clone())?;
        let current_round = Self::get_current_round(env.clone())?;
        let members = Self::get_members(env.clone())?;

        let mut candidates: Vec<(Address, i32, u32, u64)> = Vec::new(&env);

        for member_addr in members.iter() {
            let member = Self::get_member(env.clone(), member_addr.clone())?;
            let members_count = Self::get_active_members_count(env.clone())?;

            if member.can_receive(&env, current_round, members_count, &config) {
                let priority = member.priority_score(members_count, &config);
                candidates.push_back((
                    member_addr.clone(),
                    priority,
                    member.contribution_count,
                    member.joined_at,
                ));
            }
        }

        if candidates.is_empty() {
            return Ok(None);
        }

        // Sort: priority (desc) -> contribution count (desc) -> joined time (asc)
        // Note: Soroban SDK Vec has no sort method; simplified to linear max search
        let mut best: Option<(Address, i32, u32, u64)> = None;
        for candidate in candidates.iter() {
            match &best {
                None => best = Some(candidate.clone()),
                Some(current_best) => {
                    // Compare priority
                    if candidate.1 > current_best.1
                        || (candidate.1 == current_best.1 && candidate.2 > current_best.2)
                        || (candidate.1 == current_best.1
                            && candidate.2 == current_best.2
                            && candidate.3 < current_best.3)
                    {
                        best = Some(candidate.clone());
                    }
                }
            }
        }

        Ok(best.map(|b| b.0))
    }

    /// Weighted random recipient selection based on priority
    /// random_seed provided by admin/backend for pseudo-random selection
    /// Higher priority members have higher probability of being selected
    fn select_recipient_weighted(env: &Env, random_seed: u64) -> Result<Option<Address>, Error> {
        let config = Self::get_config(env.clone())?;
        let current_round = Self::get_current_round(env.clone())?;
        let members = Self::get_members(env.clone())?;

        // Collect candidates and their weights
        let mut candidates: Vec<(Address, u32)> = Vec::new(env);
        let mut total_weight = 0u64;

        for member_addr in members.iter() {
            let member = Self::get_member(env.clone(), member_addr.clone())?;
            let members_count = Self::get_active_members_count(env.clone())?;

            if member.can_receive(env, current_round, members_count, &config) {
                let priority = member.priority_score(members_count, &config);
                // weight = max(priority, 1), ensure each candidate has at least weight of 1
                let weight = if priority > 0 { priority as u32 } else { 1 };
                candidates.push_back((member_addr.clone(), weight));
                total_weight += weight as u64;
            }
        }

        if candidates.is_empty() {
            return Ok(None);
        }

        // Use random seed for weighted random selection
        let random_value = random_seed % total_weight;
        let mut accumulated_weight = 0u64;

        for candidate in candidates.iter() {
            accumulated_weight += candidate.1 as u64;
            if random_value < accumulated_weight {
                return Ok(Some(candidate.0.clone()));
            }
        }

        // Theoretically won't reach here, return last as fallback
        Ok(candidates.last().map(|c| c.0.clone()))
    }

    /// Settle round (including recipient selection and payout)
    /// Anyone can call this function (no authentication required)
    /// random_seed: Random seed for weighted random recipient selection
    pub fn settle_round(env: Env, random_seed: u64) -> Result<(), Error> {
        let config = Self::get_config(env.clone())?;

        // Check ROSCA status
        if config.status != RoscaStatus::Active {
            return Err(Error::RoscaNotActive);
        }

        let current_round = Self::get_current_round(env.clone())?;
        let members = Self::get_members(env.clone())?;
        let start_time: u64 = env.storage()
            .instance()
            .get(&DataKey::StartTime)
            .ok_or(Error::NotInitialized)?;

        // Calculate round start and end time
        let round_start = start_time + (current_round * config.contribution_period);
        let round_end = round_start + config.contribution_period;

        // Check time window: can only settle after round ends
        let current_time = env.ledger().timestamp();
        if current_time < round_end {
            return Err(Error::RoundNotEnded);
        }

        // 1. Identify violators and actual contributors
        let mut expected_contributors = Vec::new(&env);
        let mut actual_contributors = Vec::new(&env);
        let mut violators = Vec::new(&env);
        let mut total_collected = 0i128;

        for member_addr in members.iter() {
            let member = Self::get_member(env.clone(), member_addr.clone())?;

            // Both active and observing members should contribute
            if member.status == MemberStatus::Active || member.status == MemberStatus::Observing {
                expected_contributors.push_back(member_addr.clone());

                // Check if contributed in this round
                if member.last_contribution_round == current_round {
                    actual_contributors.push_back(member_addr.clone());
                    total_collected += config.contribution_amount;
                } else {
                    violators.push_back(member_addr.clone());
                }
            }
        }

        // 2. Handle violations
        let mut total_deposit_compensation = 0i128;
        for violator_addr in violators.iter() {
            let mut violator = Self::get_member(env.clone(), violator_addr.clone())?;
            violator.violation_count += 1;

            // Reset on-time streak for violators
            violator.on_time_streak = 0;

            // Check if exceeded maximum violations
            if violator.violation_count >= config.max_violations {
                // Kick out member
                violator.status = MemberStatus::Kicked;
            } else {
                // Apply progressive penalty
                let penalty_index = (violator.violation_count - 1).min((config.violation_penalties.len() - 1) as u32);
                if let Some(penalty) = config.violation_penalties.get(penalty_index) {
                    // Deduct deposit
                    let deduction = penalty.deposit_deduction.min(violator.deposit);
                    violator.deposit -= deduction;
                    total_deposit_compensation += deduction;

                    // Set lockout period
                    violator.violation_lockout_until = current_round + penalty.lockout_rounds;
                }
            }

            env.storage()
                .instance()
                .set(&DataKey::Member(violator_addr), &violator);
        }

        // 3. Weighted random selection of recipient
        let recipient_opt = Self::select_recipient_weighted(&env, random_seed)?;

        let (payout_amount, insurance_compensation, beneficiary_loss) = if let Some(recipient_addr) = &recipient_opt {
            let mut recipient = Self::get_member(env.clone(), recipient_addr.clone())?;

            // Calculate payout amount (total after insurance fee)
            let insurance_collected = (total_collected * config.insurance_rate as i128) / 100;
            let pool_amount = total_collected - insurance_collected;

            // Calculate violation loss
            let violations_loss = (violators.len() as i128) * config.contribution_amount;

            // Ideal payout amount
            let ideal_payout = pool_amount;
            let actual_available = pool_amount - violations_loss;

            // Calculate compensation
            let mut compensation_needed = if actual_available < ideal_payout {
                ideal_payout - actual_available
            } else {
                0
            };

            // Compensation from deposits
            let deposit_comp = compensation_needed.min(total_deposit_compensation);
            compensation_needed -= deposit_comp;

            // Compensation from insurance pool
            let mut insurance_pool = Self::get_insurance_pool(env.clone())?;
            let insurance_comp = compensation_needed
                .min(insurance_pool)
                .min(config.max_insurance_coverage);
            insurance_pool -= insurance_comp;
            compensation_needed -= insurance_comp;

            // Beneficiary bears remaining loss (not exceeding max loss rate)
            let max_beneficiary_loss = (ideal_payout * config.max_beneficiary_loss_rate as i128) / 100;
            let beneficiary_loss_amount = compensation_needed.min(max_beneficiary_loss);

            // Final payout amount
            let final_payout = actual_available + deposit_comp + insurance_comp;

            // Update beneficiary data
            recipient.total_received += final_payout;
            recipient.receive_count += 1;
            recipient.last_received_round = current_round;

            // Set cooldown period
            let members_count = Self::get_active_members_count(env.clone())?;
            recipient.cooldown_until_round = match config.cooldown_type {
                CooldownType::FixedRounds(rounds) => current_round + rounds,
                CooldownType::DynamicMembers => current_round + members_count as u64,
                CooldownType::TimeBased(_secs) => {
                    // Time-based cooldown needs to be converted to rounds
                    current_round + members_count as u64 // Temporarily use dynamic member count
                }
            };

            env.storage()
                .instance()
                .set(&DataKey::Member(recipient_addr.clone()), &recipient);

            // Update insurance pool
            insurance_pool += insurance_collected; // Add current round's insurance fee
            env.storage()
                .instance()
                .set(&DataKey::InsurancePool, &insurance_pool);

            // Transfer payout to recipient
            if final_payout > 0 {
                let token_client = token::Client::new(&env, &config.token_address);
                token_client.transfer(&env.current_contract_address(), recipient_addr, &final_payout);
            }

            (final_payout, insurance_comp, beneficiary_loss_amount)
        } else {
            // No eligible recipient, insurance fee goes to pool
            let insurance_collected = (total_collected * config.insurance_rate as i128) / 100;
            let mut insurance_pool = Self::get_insurance_pool(env.clone())?;
            insurance_pool += insurance_collected;
            env.storage()
                .instance()
                .set(&DataKey::InsurancePool, &insurance_pool);

            // Refund to contributors (contribution amount minus insurance fee)
            let refund_per_contributor = config.contribution_amount - (config.contribution_amount * config.insurance_rate as i128 / 100);
            if refund_per_contributor > 0 {
                let token_client = token::Client::new(&env, &config.token_address);
                for contributor_addr in actual_contributors.iter() {
                    token_client.transfer(&env.current_contract_address(), &contributor_addr, &refund_per_contributor);
                }
            }

            (0, 0, 0)
        };

        // 4. Create round record
        let violations_loss = (violators.len() as i128) * config.contribution_amount;
        let round = Round {
            round_id: current_round,
            start_time: round_start,
            end_time: round_end,
            expected_contributors: expected_contributors.clone(),
            actual_contributors: actual_contributors.clone(),
            violators: violators.clone(),
            total_collected,
            insurance_collected: (total_collected * config.insurance_rate as i128) / 100,
            recipient: recipient_opt,
            payout_amount,
            violations_loss,
            deposit_compensation: total_deposit_compensation,
            insurance_compensation,
            beneficiary_loss,
        };

        env.storage()
            .instance()
            .set(&DataKey::Round(current_round), &round);

        // 5. Update statistics
        let mut stats = Self::get_statistics(env.clone())?;
        stats.total_rounds += 1;
        stats.total_violations += violators.len() as u32;
        stats.total_paid_out += payout_amount;
        env.storage().instance().set(&DataKey::Statistics, &stats);

        // 6. Advance to next round
        env.storage()
            .instance()
            .set(&DataKey::CurrentRound, &(current_round + 1));

        Ok(())
    }

    // === Helper functions ===

    /// Validate configuration
    fn validate_config(config: &RoscaConfig) -> Result<(), Error> {
        if config.contribution_amount <= 0 {
            return Err(Error::InvalidConfig);
        }
        if config.contribution_period == 0 {
            return Err(Error::InvalidPeriod);
        }
        if config.min_deposit <= 0 || config.min_deposit > config.recommended_deposit {
            return Err(Error::InvalidDepositRange);
        }
        Ok(())
    }

    /// Get active member count
    fn get_active_members_count(env: Env) -> Result<u32, Error> {
        let members = Self::get_members(env.clone())?;
        let mut count = 0u32;
        for member_addr in members.iter() {
            let member = Self::get_member(env.clone(), member_addr)?;
            if member.status == MemberStatus::Active {
                count += 1;
            }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod test;
