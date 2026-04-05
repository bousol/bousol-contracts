#![no_std]

mod errors;
mod storage;
mod types;

use errors::Error;
use storage::{DataKey, PersistentDataKey, TempDataKey};
use types::*;

use soroban_sdk::{contract, contractimpl, token, Address, BytesN, Env, Vec};

#[contract]
pub struct RoscaV2Contract;

#[contractimpl]
impl RoscaV2Contract {
    /// Initialize the contract
    pub fn initialize(env: Env, admin: Address, config: RoscaConfig) -> Result<(), Error> {
        admin.require_auth();

        // Check if already initialized
        if env.storage().instance().has(&DataKey::Config) {
            return Err(Error::AlreadyInitialized);
        }

        // Validate configuration
        Self::validate_config(&config)?;

        // Store admin
        env.storage().instance().set(&DataKey::Admin, &admin);

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

        // Initialize proposal counter
        env.storage()
            .instance()
            .set(&DataKey::ProposalCounter, &0u64);

        // Initialize per-round insurance tracking
        env.storage()
            .instance()
            .set(&DataKey::RoundInsurance, &0i128);

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

    /// Upgrade contract WASM code (admin only)
    /// Contract address and all storage data are preserved.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        let admin: Address = env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        env.deployer().update_current_contract_wasm(new_wasm_hash);

        // Extend TTL after upgrade to ensure contract stays alive
        Self::extend_instance_ttl(&env);

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

    /// Get round information (C2: from persistent storage)
    pub fn get_round(env: Env, round_id: u64) -> Result<Round, Error> {
        env.storage()
            .persistent()
            .get(&PersistentDataKey::Round(round_id))
            .ok_or(Error::InvalidState)
    }

    /// Dissolve ROSCA and refund all members
    /// This function is now only callable via voting (execute_proposal)
    /// Uses pro-rata distribution to guarantee refunds never exceed contract balance
    fn dissolve_internal(env: Env) -> Result<(), Error> {
        let mut config = Self::get_config(env.clone())?;

        if config.status == RoscaStatus::Dissolved {
            return Err(Error::RoscaDissolved);
        }

        let members = Self::get_members(env.clone())?;
        let token_client = token::Client::new(&env, &config.token_address);

        // Get actual contract token balance
        let contract_balance = token_client.balance(&env.current_contract_address());

        // 1. Distribute insurance pool to active members first
        let insurance_pool = Self::get_insurance_pool(env.clone())?;
        let active_count = Self::get_active_members_count(env.clone())?;
        let mut insurance_distributed = 0i128;
        if insurance_pool > 0 && active_count > 0 {
            let share = insurance_pool / active_count as i128;
            // C-02 fix: cap each transfer at the actual remaining contract balance
            // to prevent panic if bookkeeping insurance_pool exceeds real balance
            let mut remaining = contract_balance;
            if share > 0 {
                for member_addr in members.iter() {
                    let member = Self::get_member(env.clone(), member_addr.clone())?;
                    if member.status == MemberStatus::Active {
                        let safe_share = share.min(remaining);
                        if safe_share > 0 {
                            token_client.transfer(
                                &env.current_contract_address(),
                                &member_addr,
                                &safe_share
                            );
                            insurance_distributed += safe_share;
                            remaining -= safe_share;
                        }
                    }
                }
            }
        }

        // 2. Calculate refund claims for all members: deposit + max(0, net_balance)
        let remaining_balance = contract_balance - insurance_distributed;
        let mut total_claims = 0i128;

        for member_addr in members.iter() {
            let member = Self::get_member(env.clone(), member_addr.clone())?;
            let mut claim = member.deposit;
            let net_balance = member.net_balance();
            if net_balance > 0 {
                claim += net_balance;
            }
            total_claims += claim;
        }

        // 3. Refund members (pro-rata if total claims exceed available balance)
        if total_claims > 0 {
            for member_addr in members.iter() {
                let member = Self::get_member(env.clone(), member_addr.clone())?;
                let mut claim = member.deposit;
                let net_balance = member.net_balance();
                if net_balance > 0 {
                    claim += net_balance;
                }

                let refund = if total_claims > remaining_balance {
                    // Pro-rata: scale down claims proportionally
                    claim * remaining_balance / total_claims
                } else {
                    claim
                };

                if refund > 0 {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &member_addr,
                        &refund
                    );
                }
            }
        }

        // Mark as dissolved
        config.status = RoscaStatus::Dissolved;
        env.storage().instance().set(&DataKey::Config, &config);

        // H4: Emit dissolution event
        env.events().publish(
            (soroban_sdk::symbol_short!("rosca"), soroban_sdk::symbol_short!("dissolve")),
            env.ledger().timestamp(),
        );

        Ok(())
    }

    /// Create a new proposal
    pub fn propose(env: Env, proposer: Address, proposal_type: ProposalType) -> Result<u64, Error> {
        proposer.require_auth();

        let config = Self::get_config(env.clone())?;

        // Check ROSCA status — allow proposing when Active or Paused (reject Dissolved)
        if config.status == RoscaStatus::Dissolved {
            return Err(Error::RoscaDissolved);
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
            ProposalType::EmergencyPayout(details) => {
                if details.amount <= 0 {
                    return Err(Error::InvalidContributionAmount);
                }
                (48 * 3600, None)  // 48 hours
            },
            ProposalType::UpdateConfig(_) => (7 * 24 * 3600, Some(7 * 24 * 3600)),  // 7d voting + 7d cooldown
            ProposalType::Dissolution(mode) => match mode {
                DissolutionMode::Emergency => (24 * 3600, None),  // 24 hours
                DissolutionMode::Normal => (14 * 24 * 3600, None), // 14 days
            },
            ProposalType::Pause => {
                // Cannot pause if already Paused
                if config.status == RoscaStatus::Paused {
                    return Err(Error::RoscaPaused);
                }
                (48 * 3600, None)  // 48 hours, no cooldown
            },
            ProposalType::Resume => {
                // Cannot resume if not Paused
                if config.status != RoscaStatus::Paused {
                    return Err(Error::NotPaused);
                }
                (48 * 3600, None)  // 48 hours, no cooldown
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

        // C2: Store proposal in persistent storage
        env.storage()
            .persistent()
            .set(&PersistentDataKey::Proposal(proposal_id), &proposal);
        Self::extend_persistent_ttl(&env, &PersistentDataKey::Proposal(proposal_id));

        // Increment proposal counter
        env.storage()
            .instance()
            .set(&DataKey::ProposalCounter, &(proposal_id + 1));

        // H4: Emit proposal created event
        env.events().publish(
            (soroban_sdk::symbol_short!("proposal"), soroban_sdk::symbol_short!("created")),
            proposal_id,
        );

        Ok(proposal_id)
    }

    /// Vote on a proposal
    pub fn vote(env: Env, voter: Address, proposal_id: u64, choice: VoteChoice) -> Result<(), Error> {
        voter.require_auth();

        let config = Self::get_config(env.clone())?;

        // Check ROSCA status — allow voting when Active or Paused (reject Dissolved)
        if config.status == RoscaStatus::Dissolved {
            return Err(Error::RoscaDissolved);
        }

        // Check voter is an active member
        let voter_member = Self::get_member(env.clone(), voter.clone())?;
        if voter_member.status != MemberStatus::Active {
            return Err(Error::MemberNotActive);
        }

        // C2: Get proposal from persistent storage
        let mut proposal = env.storage()
            .persistent()
            .get::<PersistentDataKey, Proposal>(&PersistentDataKey::Proposal(proposal_id))
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

        // C2: Check if already voted (persistent storage)
        if env.storage().persistent().has(&PersistentDataKey::Vote(proposal_id, voter.clone())) {
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

        // C2: Store vote in persistent storage
        env.storage()
            .persistent()
            .set(&PersistentDataKey::Vote(proposal_id, voter.clone()), &vote);
        Self::extend_persistent_ttl(&env, &PersistentDataKey::Vote(proposal_id, voter.clone()));

        // Update proposal vote counts
        match choice {
            VoteChoice::For => proposal.votes_for_weight += voting_weight,
            VoteChoice::Against => proposal.votes_against_weight += voting_weight,
        }

        env.storage()
            .persistent()
            .set(&PersistentDataKey::Proposal(proposal_id), &proposal);
        Self::extend_persistent_ttl(&env, &PersistentDataKey::Proposal(proposal_id));

        // H4: Emit vote cast event
        env.events().publish(
            (soroban_sdk::symbol_short!("vote"), soroban_sdk::symbol_short!("cast")),
            (proposal_id, voter),
        );

        Ok(())
    }

    /// Execute a proposal after voting ends
    pub fn execute_proposal(env: Env, executor: Address, proposal_id: u64) -> Result<(), Error> {
        executor.require_auth();

        let config = Self::get_config(env.clone())?;

        // Check ROSCA status (allow execution even for dissolution)
        if config.status == RoscaStatus::Dissolved {
            return Err(Error::RoscaDissolved);
        }

        // Check executor is a member
        let executor_member = Self::get_member(env.clone(), executor.clone())?;
        if executor_member.status != MemberStatus::Active {
            return Err(Error::MemberNotActive);
        }

        // C2: Get proposal from persistent storage
        let mut proposal = env.storage()
            .persistent()
            .get::<PersistentDataKey, Proposal>(&PersistentDataKey::Proposal(proposal_id))
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

        // H-03 fix: if total_voting_weight is 0 (no member has ever contributed),
        // no proposal can pass (0 > 0 is always false). Rather than silently deadlocking,
        // return an explicit error. Members must contribute at least once before governance works.
        if total_voting_weight == 0 {
            return Err(Error::InsufficientVotes);
        }

        // Check if proposal passes based on type
        let passes = match &proposal.proposal_type {
            ProposalType::EmergencyPayout(_) => {
                // >66% approval required (cast to u64 to prevent overflow)
                (proposal.votes_for_weight as u64) * 100 > (total_voting_weight as u64) * 66
            },
            ProposalType::UpdateConfig(_) => {
                // >50% approval required
                (proposal.votes_for_weight as u64) * 100 > (total_voting_weight as u64) * 50
            },
            ProposalType::Dissolution(mode) => {
                match mode {
                    DissolutionMode::Emergency => {
                        // >75% approval required
                        (proposal.votes_for_weight as u64) * 100 > (total_voting_weight as u64) * 75
                    },
                    DissolutionMode::Normal => {
                        // >90% approval required
                        (proposal.votes_for_weight as u64) * 100 > (total_voting_weight as u64) * 90
                    },
                }
            },
            ProposalType::Pause => {
                // >66% approval required (same as EmergencyPayout — serious action)
                (proposal.votes_for_weight as u64) * 100 > (total_voting_weight as u64) * 66
            },
            ProposalType::Resume => {
                // >50% approval required
                (proposal.votes_for_weight as u64) * 100 > (total_voting_weight as u64) * 50
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

                // Transfer emergency payout (verify contract has sufficient balance)
                let token_client = token::Client::new(&env, &config.token_address);
                let contract_balance = token_client.balance(&env.current_contract_address());
                if contract_balance < details.amount {
                    return Err(Error::InsufficientFunds);
                }
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
                requester_member.cooldown_until_round = match &config.cooldown_type {
                    CooldownType::FixedRounds(rounds) => current_round + rounds,
                    CooldownType::DynamicMembers => current_round + members_count as u64,
                    CooldownType::TimeBased(secs) => {
                        let rounds = (secs / config.contribution_period).max(1);
                        current_round + rounds
                    }
                };

                env.storage()
                    .instance()
                    .set(&DataKey::Member(details.requester.clone()), &requester_member);

                // Update statistics
                let mut stats = Self::get_statistics(env.clone())?;
                stats.total_paid_out += details.amount;
                env.storage().instance().set(&DataKey::Statistics, &stats);
            },
            ProposalType::UpdateConfig(new_config) => {
                // Validate new configuration
                Self::validate_config(new_config)?;

                // Ensure token_address is not changed
                if new_config.token_address != config.token_address {
                    return Err(Error::InvalidConfig);
                }

                // Prevent status changes via UpdateConfig (must use Dissolution proposal)
                let mut final_config = new_config.clone();
                final_config.status = config.status;

                // M1: Note — max_violations changes can retroactively kick members.
                // This is an intentional governance power: members voted for this config change.
                // The governance threshold (>50%) provides sufficient protection.

                // Replace stored configuration
                env.storage().instance().set(&DataKey::Config, &final_config);
            },
            ProposalType::Dissolution(_) => {
                // Execute dissolution
                Self::dissolve_internal(env.clone())?;
            },
            ProposalType::Pause => {
                // Cannot pause if already Paused or Dissolved
                if config.status == RoscaStatus::Paused {
                    return Err(Error::RoscaPaused);
                }
                if config.status == RoscaStatus::Dissolved {
                    return Err(Error::RoscaDissolved);
                }

                // Set status to Paused
                let mut config = config.clone();
                config.status = RoscaStatus::Paused;
                env.storage().instance().set(&DataKey::Config, &config);

                // Record pause timestamp
                env.storage().instance().set(&DataKey::PauseTime, &env.ledger().timestamp());

                // Emit pause event
                env.events().publish(
                    (soroban_sdk::symbol_short!("rosca"), soroban_sdk::symbol_short!("paused")),
                    env.ledger().timestamp(),
                );
            },
            ProposalType::Resume => {
                // Can only resume when Paused
                if config.status == RoscaStatus::Dissolved {
                    return Err(Error::RoscaDissolved);
                }
                if config.status != RoscaStatus::Paused {
                    return Err(Error::NotPaused);
                }

                // Adjust StartTime to account for pause duration
                let pause_time: u64 = env.storage()
                    .instance()
                    .get(&DataKey::PauseTime)
                    .ok_or(Error::InvalidState)?;
                let current_time = env.ledger().timestamp();
                let pause_duration = current_time - pause_time;

                let start_time: u64 = env.storage()
                    .instance()
                    .get(&DataKey::StartTime)
                    .ok_or(Error::NotInitialized)?;
                let new_start_time = start_time + pause_duration;
                env.storage().instance().set(&DataKey::StartTime, &new_start_time);

                // Set status to Active
                let mut config = config.clone();
                config.status = RoscaStatus::Active;
                env.storage().instance().set(&DataKey::Config, &config);

                // Remove PauseTime
                env.storage().instance().remove(&DataKey::PauseTime);

                // Emit resume event
                env.events().publish(
                    (soroban_sdk::symbol_short!("rosca"), soroban_sdk::symbol_short!("resumed")),
                    env.ledger().timestamp(),
                );
            },
        }

        // Mark proposal as executed
        proposal.executed = true;
        env.storage()
            .persistent()
            .set(&PersistentDataKey::Proposal(proposal_id), &proposal);
        Self::extend_persistent_ttl(&env, &PersistentDataKey::Proposal(proposal_id));

        // H4: Emit proposal executed event
        env.events().publish(
            (soroban_sdk::symbol_short!("proposal"), soroban_sdk::symbol_short!("exec")),
            proposal_id,
        );

        Ok(())
    }

    /// Get proposal information (C2: from persistent storage)
    pub fn get_proposal(env: Env, proposal_id: u64) -> Result<Proposal, Error> {
        env.storage()
            .persistent()
            .get(&PersistentDataKey::Proposal(proposal_id))
            .ok_or(Error::ProposalNotFound)
    }

    /// Get vote record (C2: from persistent storage)
    pub fn get_vote(env: Env, proposal_id: u64, voter: Address) -> Result<Vote, Error> {
        env.storage()
            .persistent()
            .get(&PersistentDataKey::Vote(proposal_id, voter))
            .ok_or(Error::InvalidState)
    }

    /// Member join
    pub fn join(env: Env, member: Address, deposit_amount: i128) -> Result<(), Error> {
        member.require_auth();

        let config: RoscaConfig = Self::get_config(env.clone())?;

        // Check ROSCA status
        if config.status == RoscaStatus::Paused {
            return Err(Error::RoscaPaused);
        }
        if config.status != RoscaStatus::Active {
            return Err(Error::RoscaNotActive);
        }

        // Check if joining is allowed
        if !config.allow_join {
            return Err(Error::JoinNotAllowed);
        }

        // Check if group is full
        let members = Self::get_members(env.clone())?;
        if members.len() >= config.max_members {
            return Err(Error::GroupFull);
        }

        // Check if member already exists
        if env.storage().instance().has(&DataKey::Member(member.clone())) {
            return Err(Error::MemberAlreadyExists);
        }

        // Check sponsor requirement and read sponsor address (C2: from temporary storage)
        let sponsored_by: Option<Address> = if config.require_sponsor {
            let sponsor: Address = env.storage().temporary()
                .get(&TempDataKey::Sponsor(member.clone()))
                .ok_or(Error::SponsorRequired)?;
            // Clean up temporary sponsor record (audit trail stored in Member)
            env.storage().temporary().remove(&TempDataKey::Sponsor(member.clone()));
            Some(sponsor)
        } else {
            // Even without require_sponsor, check if a voluntary sponsor record exists
            let sponsor: Option<Address> = env.storage().temporary()
                .get(&TempDataKey::Sponsor(member.clone()));
            if sponsor.is_some() {
                env.storage().temporary().remove(&TempDataKey::Sponsor(member.clone()));
            }
            sponsor
        };

        // Check deposit amount
        if deposit_amount < config.min_deposit {
            return Err(Error::InsufficientDeposit);
        }
        // ExceedsMaxDeposit: use correct error for exceeding max
        if let Some(max_deposit) = config.max_deposit {
            if deposit_amount > max_deposit {
                return Err(Error::ExceedsMaxDeposit);
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
            last_contribution_round: u64::MAX, // M3: Sentinel — never contributed yet. Only used in == comparison, safe.
            last_received_round: u64::MAX,     // M3: Sentinel — never received yet. Only used in == comparison, safe.
            cooldown_until_round: 0,
            violation_lockout_until: 0,
            sponsored_by,
        };

        // Store member
        env.storage()
            .instance()
            .set(&DataKey::Member(member.clone()), &new_member);

        // Add to members list (reuse the members list fetched earlier for max_members check)
        let mut members = members;
        members.push_back(member.clone());
        env.storage().instance().set(&DataKey::MembersList, &members);

        // Update statistics
        let mut stats = Self::get_statistics(env.clone())?;
        stats.total_members += 1;
        if !config.all_members_observation {
            stats.active_members += 1;
        }
        env.storage().instance().set(&DataKey::Statistics, &stats);

        // Transfer deposit from member to contract
        let token_client = token::Client::new(&env, &config.token_address);
        token_client.transfer(&member, &env.current_contract_address(), &deposit_amount);

        // H4: Emit member joined event
        env.events().publish(
            (soroban_sdk::symbol_short!("member"), soroban_sdk::symbol_short!("joined")),
            member,
        );

        Ok(())
    }

    /// Sponsor a candidate for joining (sponsor must be an Active member)
    pub fn sponsor(env: Env, sponsor: Address, candidate: Address) -> Result<(), Error> {
        sponsor.require_auth();

        let config = Self::get_config(env.clone())?;

        if config.status == RoscaStatus::Paused {
            return Err(Error::RoscaPaused);
        }
        if config.status != RoscaStatus::Active {
            return Err(Error::RoscaNotActive);
        }

        // Sponsor must be an active member
        let sponsor_member = Self::get_member(env.clone(), sponsor.clone())?;
        if sponsor_member.status != MemberStatus::Active {
            return Err(Error::MemberNotActive);
        }

        // Candidate must not already be a member
        if env.storage().instance().has(&DataKey::Member(candidate.clone())) {
            return Err(Error::MemberAlreadyExists);
        }

        // M5: Check if sponsor record already exists — prevent silent overwrite
        if env.storage().temporary().has(&TempDataKey::Sponsor(candidate.clone())) {
            return Err(Error::SponsorAlreadyExists);
        }

        // C2: Store sponsor record in temporary storage (auto-expires)
        let sponsor_key = TempDataKey::Sponsor(candidate);
        env.storage()
            .temporary()
            .set(&sponsor_key, &sponsor);
        // TTL for sponsor records: ~7 days
        const WEEK_IN_LEDGERS: u32 = 120_960;
        env.storage()
            .temporary()
            .extend_ttl(&sponsor_key, WEEK_IN_LEDGERS / 2, WEEK_IN_LEDGERS);

        Ok(())
    }

    /// Top up deposit (replenish after violation deductions)
    /// NOTE: Intentionally allows top-up during Paused status. This is by design — members
    /// should be able to replenish their deposit (e.g. after violation deductions) even while
    /// the ROSCA is paused, so they are ready when it resumes. Only Dissolved is blocked.
    pub fn top_up_deposit(env: Env, member: Address, amount: i128) -> Result<(), Error> {
        member.require_auth();

        let config = Self::get_config(env.clone())?;

        if config.status == RoscaStatus::Dissolved {
            return Err(Error::RoscaDissolved);
        }

        let mut member_data = Self::get_member(env.clone(), member.clone())?;

        // Only Active or Observing members can top up
        if member_data.status != MemberStatus::Active && member_data.status != MemberStatus::Observing {
            return Err(Error::MemberNotActive);
        }

        // Validate amount
        if amount <= 0 {
            return Err(Error::InvalidContributionAmount);
        }

        // Check max deposit limit — use ExceedsMaxDeposit error
        if let Some(max_deposit) = config.max_deposit {
            if member_data.deposit + amount > max_deposit {
                return Err(Error::ExceedsMaxDeposit);
            }
        }

        // Transfer tokens from member to contract
        let token_client = token::Client::new(&env, &config.token_address);
        token_client.transfer(&member, &env.current_contract_address(), &amount);

        // Update member deposit
        member_data.deposit += amount;
        env.storage()
            .instance()
            .set(&DataKey::Member(member), &member_data);

        Ok(())
    }

    /// Request exit (two-step: sets ExitPending, actual exit happens at settle_round)
    pub fn request_exit(env: Env, member: Address) -> Result<(), Error> {
        member.require_auth();

        // H-04 fix: block exit requests when ROSCA is dissolved (members already refunded).
        // Exit during Paused is intentionally allowed (members may want to leave a paused group).
        let config = Self::get_config(env.clone())?;
        if config.status == RoscaStatus::Dissolved {
            return Err(Error::RoscaDissolved);
        }

        let mut member_data = Self::get_member(env.clone(), member.clone())?;

        // Only Active or Observing members can request exit
        if member_data.status != MemberStatus::Active && member_data.status != MemberStatus::Observing {
            return Err(Error::MemberNotActive);
        }

        // Check if member can exit (net_balance >= 0)
        if !member_data.can_exit() {
            return Err(Error::CannotExit);
        }

        // Decrement active_members if member was Active (Observing members were never counted)
        let was_active = member_data.status == MemberStatus::Active;

        // Set status to ExitPending — actual exit happens at settle_round
        member_data.status = MemberStatus::ExitPending;
        env.storage()
            .instance()
            .set(&DataKey::Member(member.clone()), &member_data);

        if was_active {
            let mut stats = Self::get_statistics(env.clone())?;
            stats.active_members = stats.active_members.saturating_sub(1);
            env.storage().instance().set(&DataKey::Statistics, &stats);
        }

        // H4: Emit member exited event
        env.events().publish(
            (soroban_sdk::symbol_short!("member"), soroban_sdk::symbol_short!("exit")),
            member,
        );

        Ok(())
    }

    /// Contribution
    pub fn contribute(env: Env, member: Address) -> Result<(), Error> {
        member.require_auth();

        let config = Self::get_config(env.clone())?;

        // Check ROSCA status
        if config.status == RoscaStatus::Paused {
            return Err(Error::RoscaPaused);
        }
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

        // Update member data
        member_data.total_contributed += config.contribution_amount;
        member_data.contribution_count += 1;
        member_data.last_contribution_round = current_round;

        // Increment on-time streak for on-time contributions
        member_data.on_time_streak += 1;

        // Update observation count
        let mut promoted = false;
        if member_data.status == MemberStatus::Observing {
            member_data.observation_count += 1;
            if member_data.observation_count >= config.observation_contributions {
                member_data.status = MemberStatus::Active;
                promoted = true;
            }
        }

        env.storage()
            .instance()
            .set(&DataKey::Member(member.clone()), &member_data);

        // H1: Update insurance pool (capped at max — track actual insurance added)
        let mut insurance_pool = Self::get_insurance_pool(env.clone())?;
        let actual_insurance = insurance_amount.min(config.max_insurance_pool - insurance_pool).max(0);
        insurance_pool += actual_insurance;
        env.storage()
            .instance()
            .set(&DataKey::InsurancePool, &insurance_pool);

        // H1: Track per-round actual insurance
        let mut round_insurance: i128 = env.storage()
            .instance()
            .get(&DataKey::RoundInsurance)
            .unwrap_or(0);
        round_insurance += actual_insurance;
        env.storage()
            .instance()
            .set(&DataKey::RoundInsurance, &round_insurance);

        // Update statistics
        let mut stats = Self::get_statistics(env.clone())?;
        stats.total_contributed += config.contribution_amount;
        stats.insurance_pool = insurance_pool;
        if promoted {
            stats.active_members += 1;
        }
        env.storage().instance().set(&DataKey::Statistics, &stats);

        // Transfer contribution amount from member to contract
        let token_client = token::Client::new(&env, &config.token_address);
        token_client.transfer(&member, &env.current_contract_address(), &config.contribution_amount);

        // Extend storage TTL on every contribution
        Self::extend_instance_ttl(&env);

        // H4: Emit contribution event
        env.events().publish(
            (soroban_sdk::symbol_short!("contrib"), soroban_sdk::symbol_short!("made")),
            (member, current_round),
        );

        Ok(())
    }

    /// Late contribution (with late fee)
    pub fn contribute_late(env: Env, member: Address) -> Result<(), Error> {
        member.require_auth();

        let config = Self::get_config(env.clone())?;

        // Check ROSCA status
        if config.status == RoscaStatus::Paused {
            return Err(Error::RoscaPaused);
        }
        if config.status != RoscaStatus::Active {
            return Err(Error::RoscaNotActive);
        }

        let mut member_data = Self::get_member(env.clone(), member.clone())?;

        // Check member status (must be Active or Observing)
        if member_data.status != MemberStatus::Active && member_data.status != MemberStatus::Observing {
            return Err(Error::MemberNotActive);
        }

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

        // Check if already contributed this round
        if member_data.last_contribution_round == current_round {
            return Err(Error::AlreadyContributed);
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
        let mut promoted = false;
        if member_data.status == MemberStatus::Observing {
            member_data.observation_count += 1;
            if member_data.observation_count >= config.observation_contributions {
                member_data.status = MemberStatus::Active;
                promoted = true;
            }
        }

        env.storage()
            .instance()
            .set(&DataKey::Member(member.clone()), &member_data);

        // H1: Update insurance pool (normal insurance + late fee, capped at max — only track what fits)
        let mut insurance_pool = Self::get_insurance_pool(env.clone())?;
        let total_insurance = insurance_amount + late_fee;
        let actual_insurance = total_insurance.min(config.max_insurance_pool - insurance_pool).max(0);
        insurance_pool += actual_insurance;
        env.storage()
            .instance()
            .set(&DataKey::InsurancePool, &insurance_pool);

        // H1: Track per-round actual insurance
        let mut round_insurance: i128 = env.storage()
            .instance()
            .get(&DataKey::RoundInsurance)
            .unwrap_or(0);
        round_insurance += actual_insurance;
        env.storage()
            .instance()
            .set(&DataKey::RoundInsurance, &round_insurance);

        // Update statistics
        let mut stats = Self::get_statistics(env.clone())?;
        stats.total_contributed += config.contribution_amount;
        stats.insurance_pool = insurance_pool;
        if promoted {
            stats.active_members += 1;
        }
        env.storage().instance().set(&DataKey::Statistics, &stats);

        // Transfer total amount (contribution + late fee) from member to contract
        let token_client = token::Client::new(&env, &config.token_address);
        token_client.transfer(&member, &env.current_contract_address(), &total_amount);

        // H4: Emit late contribution event
        env.events().publish(
            (soroban_sdk::symbol_short!("contrib"), soroban_sdk::symbol_short!("late")),
            (member, current_round),
        );

        Ok(())
    }

    /// Calculate recipient for current round (priority-based, deterministic query)
    /// This function is for querying who has highest priority, non-consuming, for reference only
    pub fn calculate_recipient(env: Env) -> Result<Option<Address>, Error> {
        let config = Self::get_config(env.clone())?;
        let current_round = Self::get_current_round(env.clone())?;
        let members = Self::get_members(env.clone())?;

        let mut candidates: Vec<(Address, i64, u32, u64)> = Vec::new(&env);

        // M-05 fix: compute active members count ONCE before the loop (O(n) instead of O(n^2))
        let members_count = Self::get_active_members_count(env.clone())?;

        for member_addr in members.iter() {
            let member = Self::get_member(env.clone(), member_addr.clone())?;

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
        let mut best: Option<(Address, i64, u32, u64)> = None;
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
    /// Uses on-chain PRNG for secure randomness
    /// Higher priority members have higher probability of being selected
    fn select_recipient_weighted(env: &Env) -> Result<Option<Address>, Error> {
        let config = Self::get_config(env.clone())?;
        let current_round = Self::get_current_round(env.clone())?;
        let members = Self::get_members(env.clone())?;

        // Collect candidates and their weights
        // C-01 fix: use u64 for weights to avoid truncating i64 priority scores
        let mut candidates: Vec<(Address, u64)> = Vec::new(env);
        let mut total_weight = 0u64;

        // M-05 fix: compute active members count ONCE before the loop (O(n) instead of O(n^2))
        let members_count = Self::get_active_members_count(env.clone())?;

        for member_addr in members.iter() {
            let member = Self::get_member(env.clone(), member_addr.clone())?;

            if member.can_receive(env, current_round, members_count, &config) {
                let priority = member.priority_score(members_count, &config);
                // can_receive requires priority > 0, so cast to u64 is safe
                let weight = priority as u64;
                candidates.push_back((member_addr.clone(), weight));
                total_weight += weight;
            }
        }

        if candidates.is_empty() {
            return Ok(None);
        }

        // Use on-chain PRNG for secure random selection
        let random_value = env.prng().gen_range::<u64>(0..total_weight);
        let mut accumulated_weight = 0u64;

        for candidate in candidates.iter() {
            accumulated_weight += candidate.1;
            if random_value < accumulated_weight {
                return Ok(Some(candidate.0.clone()));
            }
        }

        // Theoretically won't reach here, return last as fallback
        Ok(candidates.last().map(|c| c.0.clone()))
    }

    /// Settle round (including recipient selection and payout)
    /// C3: Permissionless — anyone can call after round ends + grace period
    pub fn settle_round(env: Env) -> Result<(), Error> {
        // C3: No admin auth required. The time check ensures it can't be called early.
        let config = Self::get_config(env.clone())?;

        // Check ROSCA status
        if config.status == RoscaStatus::Paused {
            return Err(Error::RoscaPaused);
        }
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

        // Check time window: can only settle after round ends + grace period
        // (members may use contribute_late during the grace period)
        let current_time = env.ledger().timestamp();
        let settle_after = round_end + config.violation_grace_period;
        if current_time < settle_after {
            return Err(Error::RoundNotEnded);
        }

        // 1. Identify violators and actual contributors
        let mut expected_contributors = Vec::new(&env);
        let mut actual_contributors = Vec::new(&env);
        let mut violators = Vec::new(&env);
        let mut total_collected = 0i128;

        for member_addr in members.iter() {
            let member = Self::get_member(env.clone(), member_addr.clone())?;

            // Active and Observing members should contribute; ExitPending members are excluded
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
        let mut kicked_count = 0u32;
        for violator_addr in violators.iter() {
            let mut violator = Self::get_member(env.clone(), violator_addr.clone())?;
            violator.violation_count += 1;

            // Reset on-time streak for violators
            violator.on_time_streak = 0;

            // Check if exceeded maximum violations
            if violator.violation_count >= config.max_violations {
                // Kick out member — confiscate all remaining deposit for compensation
                let deduction = violator.deposit;
                violator.deposit = 0;
                total_deposit_compensation += deduction;
                // Only count Active members for active_members decrement
                // (Observing members were never counted in active_members)
                if violator.status == MemberStatus::Active {
                    kicked_count += 1;
                }
                violator.status = MemberStatus::Kicked;

                // H4: Emit member kicked event
                env.events().publish(
                    (soroban_sdk::symbol_short!("member"), soroban_sdk::symbol_short!("kicked")),
                    violator_addr.clone(),
                );
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
        let recipient_opt = Self::select_recipient_weighted(&env)?;

        // H1: Read actual insurance collected this round (from contribute/contribute_late)
        let actual_round_insurance: i128 = env.storage()
            .instance()
            .get(&DataKey::RoundInsurance)
            .unwrap_or(0);

        let (payout_amount, insurance_compensation, beneficiary_loss) = if let Some(recipient_addr) = &recipient_opt {
            let mut recipient = Self::get_member(env.clone(), recipient_addr.clone())?;

            // H1: Use actual insurance collected (not theoretical rate) for pool_amount calculation
            // This correctly handles insurance pool overflow — excess stays in payout pool
            let pool_amount = total_collected - actual_round_insurance;

            // C1 FIX: pool_amount already only reflects actual contributors (violators didn't contribute).
            // The violation loss is already reflected by the fact that violators are NOT in total_collected.
            // actual_available IS pool_amount — no double subtraction.
            let actual_available = pool_amount;

            // C1 FIX: ideal_full_payout = what recipient would get if ALL expected members contributed
            let ideal_full_payout = (expected_contributors.len() as i128) * config.contribution_amount
                * (100 - config.insurance_rate as i128) / 100;

            // C1 FIX: compensation_needed = gap between ideal and actual
            let mut compensation_needed = if ideal_full_payout > actual_available {
                ideal_full_payout - actual_available
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
            let max_beneficiary_loss = (ideal_full_payout * config.max_beneficiary_loss_rate as i128) / 100;
            let beneficiary_loss_amount = compensation_needed.min(max_beneficiary_loss);

            // Final payout amount (ensure non-negative)
            let final_payout = (actual_available + deposit_comp + insurance_comp).max(0);

            // Update beneficiary data
            recipient.total_received += final_payout;
            recipient.receive_count += 1;
            recipient.last_received_round = current_round;

            // Set cooldown period
            let members_count = Self::get_active_members_count(env.clone())?;
            recipient.cooldown_until_round = match &config.cooldown_type {
                CooldownType::FixedRounds(rounds) => current_round + rounds,
                CooldownType::DynamicMembers => current_round + members_count as u64,
                CooldownType::TimeBased(secs) => {
                    let rounds = (secs / config.contribution_period).max(1);
                    current_round + rounds
                }
            };

            env.storage()
                .instance()
                .set(&DataKey::Member(recipient_addr.clone()), &recipient);

            // Store updated insurance pool (insurance was already added in contribute/contribute_late;
            // here we only subtracted insurance_comp for compensation)
            env.storage()
                .instance()
                .set(&DataKey::InsurancePool, &insurance_pool);

            // Transfer payout to recipient (cap at actual contract balance to prevent bricking)
            if final_payout > 0 {
                let token_client = token::Client::new(&env, &config.token_address);
                let contract_balance = token_client.balance(&env.current_contract_address());
                let safe_payout = final_payout.min(contract_balance);
                if safe_payout > 0 {
                    token_client.transfer(&env.current_contract_address(), recipient_addr, &safe_payout);
                }
                // Update total_received to reflect actual payout if capped
                if safe_payout < final_payout {
                    recipient.total_received -= final_payout - safe_payout;
                    env.storage()
                        .instance()
                        .set(&DataKey::Member(recipient_addr.clone()), &recipient);
                }

                // H4: Emit payout event
                env.events().publish(
                    (soroban_sdk::symbol_short!("payout"), soroban_sdk::symbol_short!("made")),
                    (recipient_addr.clone(), safe_payout),
                );

                (safe_payout, insurance_comp, beneficiary_loss_amount)
            } else {
                (0, insurance_comp, beneficiary_loss_amount)
            }
        } else {
            // No eligible recipient — insurance was already added to pool in contribute/contribute_late

            // Refund to contributors (contribution amount minus actual insurance per contributor)
            let actual_insurance_per_contributor = if actual_contributors.len() > 0 {
                actual_round_insurance / actual_contributors.len() as i128
            } else {
                0
            };
            let refund_per_contributor = config.contribution_amount - actual_insurance_per_contributor;
            if refund_per_contributor > 0 {
                let token_client = token::Client::new(&env, &config.token_address);
                for contributor_addr in actual_contributors.iter() {
                    token_client.transfer(&env.current_contract_address(), &contributor_addr, &refund_per_contributor);
                }
            }

            (0, 0, 0)
        };

        // Calculate violations_loss for the record
        let violations_loss = (violators.len() as i128) * config.contribution_amount;

        // 4. Create round record (C2: store in persistent storage)
        let round = Round {
            round_id: current_round,
            start_time: round_start,
            end_time: round_end,
            expected_contributors: expected_contributors.clone(),
            actual_contributors: actual_contributors.clone(),
            violators: violators.clone(),
            total_collected,
            insurance_collected: actual_round_insurance,
            recipient: recipient_opt,
            payout_amount,
            actual_insurance: actual_round_insurance,
            violations_loss,
            deposit_compensation: total_deposit_compensation,
            insurance_compensation,
            beneficiary_loss,
        };

        env.storage()
            .persistent()
            .set(&PersistentDataKey::Round(current_round), &round);
        Self::extend_persistent_ttl(&env, &PersistentDataKey::Round(current_round));

        // 5. Update statistics
        let mut stats = Self::get_statistics(env.clone())?;
        stats.total_rounds += 1;
        stats.total_violations += violators.len() as u32;
        stats.total_paid_out += payout_amount;
        stats.active_members = stats.active_members.saturating_sub(kicked_count);
        env.storage().instance().set(&DataKey::Statistics, &stats);

        // 6. Advance to next round and reset per-round insurance tracking
        env.storage()
            .instance()
            .set(&DataKey::CurrentRound, &(current_round + 1));
        env.storage()
            .instance()
            .set(&DataKey::RoundInsurance, &0i128);

        // 7. Process ExitPending and Kicked members — refund exiting, remove both from list
        let updated_members = Self::get_members(env.clone())?;
        let mut exit_claims: Vec<(Address, i128)> = Vec::new(&env);
        let mut kicked_addrs: Vec<Address> = Vec::new(&env);
        let mut remaining_members = Vec::new(&env);
        let mut total_exit_claims = 0i128;
        let mut list_changed = false;

        for member_addr in updated_members.iter() {
            let m = Self::get_member(env.clone(), member_addr.clone())?;
            if m.status == MemberStatus::ExitPending {
                let mut claim = m.deposit;
                let net_balance = m.net_balance();
                if net_balance > 0 {
                    claim += net_balance;
                }
                total_exit_claims += claim;
                exit_claims.push_back((member_addr.clone(), claim));
                list_changed = true;
            } else if m.status == MemberStatus::Kicked {
                // Kicked members: deposit already confiscated, just remove from list
                kicked_addrs.push_back(member_addr.clone());
                list_changed = true;
            } else {
                remaining_members.push_back(member_addr);
            }
        }

        // Process exit refunds
        if !exit_claims.is_empty() {
            let token_client = token::Client::new(&env, &config.token_address);
            let available_balance = token_client.balance(&env.current_contract_address());

            for exit in exit_claims.iter() {
                let member_addr = exit.0.clone();
                let claim = exit.1;

                let refund = if total_exit_claims > available_balance && total_exit_claims > 0 {
                    // Pro-rata: scale down claims proportionally to prevent panic
                    claim * available_balance / total_exit_claims
                } else {
                    claim
                };

                if refund > 0 {
                    token_client.transfer(&env.current_contract_address(), &member_addr, &refund);
                }

                // Remove member data
                env.storage().instance().remove(&DataKey::Member(member_addr));
            }
        }

        // Clean up kicked member data
        for kicked_addr in kicked_addrs.iter() {
            env.storage().instance().remove(&DataKey::Member(kicked_addr));
        }

        // Update members list if any were removed
        if list_changed {
            env.storage().instance().set(&DataKey::MembersList, &remaining_members);
        }

        // Extend storage TTL on every settlement
        Self::extend_instance_ttl(&env);

        // H4: Emit round settled event
        env.events().publish(
            (soroban_sdk::symbol_short!("round"), soroban_sdk::symbol_short!("settled")),
            current_round,
        );

        Ok(())
    }

    // === Helper functions ===

    /// Extend instance TTL to prevent storage expiry
    /// Soroban instance storage shares TTL with the contract instance.
    /// We extend to ~30 days (ledger closes ~every 5 seconds, ~518400 ledgers/month)
    fn extend_instance_ttl(env: &Env) {
        const MONTH_IN_LEDGERS: u32 = 518_400;  // ~30 days
        const THRESHOLD: u32 = MONTH_IN_LEDGERS / 2;  // Extend when < 15 days remaining
        env.storage()
            .instance()
            .extend_ttl(THRESHOLD, MONTH_IN_LEDGERS);
    }

    /// C2: Extend persistent storage TTL for historical data
    fn extend_persistent_ttl(env: &Env, key: &PersistentDataKey) {
        const YEAR_IN_LEDGERS: u32 = 6_307_200;  // ~365 days
        const THRESHOLD: u32 = YEAR_IN_LEDGERS / 2;
        env.storage()
            .persistent()
            .extend_ttl(key, THRESHOLD, YEAR_IN_LEDGERS);
    }

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
        // M2: Insurance rate must be < 50% (>= 50 is economically nonsensical)
        if config.insurance_rate >= 50 {
            return Err(Error::InvalidConfig);
        }
        // Beneficiary loss rate must be a valid percentage (0-100)
        if config.max_beneficiary_loss_rate > 100 {
            return Err(Error::InvalidConfig);
        }
        // Max deposit (if set) must be >= min_deposit
        if let Some(max_deposit) = config.max_deposit {
            if max_deposit < config.min_deposit {
                return Err(Error::InvalidDepositRange);
            }
        }
        // Validate recommended_deposit <= max_deposit when max_deposit is set
        if let Some(max_deposit) = config.max_deposit {
            if config.recommended_deposit > max_deposit {
                return Err(Error::InvalidDepositRange);
            }
        }
        // violation_penalties must not be empty (used for progressive penalties in settle_round)
        if config.violation_penalties.len() == 0 {
            return Err(Error::InvalidConfig);
        }
        // M1: max_violations must be >= 1
        if config.max_violations < 1 {
            return Err(Error::InvalidConfig);
        }
        // Validate late_fee_rates is non-empty when max_late_count > 0
        if config.max_late_count > 0 && config.late_fee_rates.len() == 0 {
            return Err(Error::InvalidConfig);
        }
        // max_members must be between 2 and 100
        if config.max_members < 2 || config.max_members > 100 {
            return Err(Error::InvalidConfig);
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
