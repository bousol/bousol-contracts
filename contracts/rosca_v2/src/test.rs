#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::{Address as _, Ledger}, token::StellarAssetClient, Address, Env};

#[test]
fn test_initialize() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let token_address = Address::generate(&env);

    let config = RoscaConfig {
        contribution_amount: 1_000_000_000,
        contribution_period: 604_800,
        min_deposit: 3_000_000_000,
        recommended_deposit: 5_000_000_000,
        max_deposit: Some(10_000_000_000),
        insurance_rate: 2,
        max_insurance_pool: 20_000_000_000,
        max_insurance_coverage: 2_000_000_000,
        observation_contributions: 3,
        all_members_observation: true,
        cooldown_type: CooldownType::DynamicMembers,
        violation_grace_period: 86_400,
        violation_penalties: soroban_sdk::vec![
            &env,
            ViolationPenalty {
                deposit_deduction: 1_000_000_000,
                points_deduction: 3,
                lockout_rounds: 2,
            },
            ViolationPenalty {
                deposit_deduction: 2_000_000_000,
                points_deduction: 5,
                lockout_rounds: 5,
            },
        ],
        max_violations: 3,
        late_fee_rates: soroban_sdk::vec![&env, 5, 10, 20],
        max_late_count: 3,
        max_beneficiary_loss_rate: 10,
        allow_join: true,
        require_sponsor: false,
        status: RoscaStatus::Active,
        token_address: token_address.clone(),
    };

    client.initialize(&config);

    let retrieved_config = client.get_config();
    assert_eq!(retrieved_config.contribution_amount, 1_000_000_000);
    assert_eq!(retrieved_config.allow_join, true);
}

#[test]
fn test_join() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);

    // Register a mock token contract
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    // Mint tokens to member1 for deposit
    token_client.mint(&member1, &10_000_000_000);

    let config = RoscaConfig {
        contribution_amount: 1_000_000_000,
        contribution_period: 604_800,
        min_deposit: 3_000_000_000,
        recommended_deposit: 5_000_000_000,
        max_deposit: Some(10_000_000_000),
        insurance_rate: 2,
        max_insurance_pool: 20_000_000_000,
        max_insurance_coverage: 2_000_000_000,
        observation_contributions: 3,
        all_members_observation: true,
        cooldown_type: CooldownType::DynamicMembers,
        violation_grace_period: 86_400,
        violation_penalties: soroban_sdk::vec![
            &env,
            ViolationPenalty {
                deposit_deduction: 1_000_000_000,
                points_deduction: 3,
                lockout_rounds: 2,
            },
        ],
        max_violations: 3,
        late_fee_rates: soroban_sdk::vec![&env, 5, 10, 20],
        max_late_count: 3,
        max_beneficiary_loss_rate: 10,
        allow_join: true,
        require_sponsor: false,
        status: RoscaStatus::Active,
        token_address: token_address.clone(),
    };

    client.initialize(&config);

    client.join(&member1, &5_000_000_000);

    let member_data = client.get_member(&member1);
    assert_eq!(member_data.deposit, 5_000_000_000);
    assert_eq!(member_data.status, MemberStatus::Observing);
}

#[test]
fn test_priority_score() {
    let env = Env::default();
    let token_address = Address::generate(&env);

    let config = RoscaConfig {
        contribution_amount: 1_000_000_000,
        contribution_period: 604_800,
        min_deposit: 3_000_000_000,
        recommended_deposit: 5_000_000_000,
        max_deposit: Some(10_000_000_000),
        insurance_rate: 2,
        max_insurance_pool: 20_000_000_000,
        max_insurance_coverage: 2_000_000_000,
        observation_contributions: 3,
        all_members_observation: true,
        cooldown_type: CooldownType::DynamicMembers,
        violation_grace_period: 86_400,
        violation_penalties: soroban_sdk::vec![
            &env,
            ViolationPenalty {
                deposit_deduction: 1_000_000_000,
                points_deduction: 3,
                lockout_rounds: 2,
            },
        ],
        max_violations: 3,
        late_fee_rates: soroban_sdk::vec![&env, 5, 10, 20],
        max_late_count: 3,
        max_beneficiary_loss_rate: 10,
        allow_join: true,
        require_sponsor: false,
        status: RoscaStatus::Active,
        token_address: token_address.clone(),
    };

    let member = Member {
        address: Address::generate(&env),
        joined_at: 0,
        status: MemberStatus::Active,
        is_system_account: false,
        deposit: 5_000_000_000,
        total_contributed: 12_000_000_000,
        total_received: 10_000_000_000,
        contribution_count: 12,
        receive_count: 1,
        violation_count: 0,
        late_count: 0,
        on_time_streak: 5,
        observation_count: 3,
        last_contribution_round: 12,
        last_received_round: 4,
        cooldown_until_round: 0,
        violation_lockout_until: 0,
        sponsored_by: None,
    };

    let priority = member.priority_score(10, &config);
    // 12 - (1 * 10) - 0 (violation_penalty) = 2
    assert_eq!(priority, 2);
}

#[test]
fn test_settle_round_with_violation() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);
    let member3 = Address::generate(&env);

    // Register a mock token contract
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    // Mint tokens to members for deposit and contributions
    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&member2, &50_000_000_000);
    token_client.mint(&member3, &50_000_000_000);

    let config = RoscaConfig {
        contribution_amount: 1_000_000_000,
        contribution_period: 604_800,
        min_deposit: 3_000_000_000,
        recommended_deposit: 5_000_000_000,
        max_deposit: Some(10_000_000_000),
        insurance_rate: 2,
        max_insurance_pool: 20_000_000_000,
        max_insurance_coverage: 2_000_000_000,
        observation_contributions: 0, // skip observation period for testing
        all_members_observation: false,
        cooldown_type: CooldownType::DynamicMembers,
        violation_grace_period: 86_400,
        violation_penalties: soroban_sdk::vec![
            &env,
            ViolationPenalty {
                deposit_deduction: 1_000_000_000,
                points_deduction: 3,
                lockout_rounds: 2,
            },
            ViolationPenalty {
                deposit_deduction: 2_000_000_000,
                points_deduction: 5,
                lockout_rounds: 5,
            },
        ],
        max_violations: 3,
        late_fee_rates: soroban_sdk::vec![&env, 5, 10, 20],
        max_late_count: 3,
        max_beneficiary_loss_rate: 10,
        allow_join: true,
        require_sponsor: false,
        status: RoscaStatus::Active,
        token_address: token_address.clone(),
    };

    client.initialize(&config);

    // Three members join
    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);
    client.join(&member3, &5_000_000_000);

    // member1 and member2 contribute, member3 violates
    client.contribute(&member1);
    client.contribute(&member2);

    // Advance time past round end + grace period
    env.ledger().with_mut(|li| {
        li.timestamp += 604_800 + 86_400 + 1; // contribution_period + grace_period
    });

    // Settle round (anyone can call)
    client.settle_round(&12345u64);

    // Check round data
    let round = client.get_round(&0);
    assert_eq!(round.round_id, 0);
    assert_eq!(round.actual_contributors.len(), 2);
    assert_eq!(round.violators.len(), 1);
    assert_eq!(round.total_collected, 2_000_000_000); // only 2 members contributed

    // Check member3 violation record
    let member3_data = client.get_member(&member3);
    assert_eq!(member3_data.violation_count, 1);
    assert_eq!(member3_data.deposit, 4_000_000_000); // 5000 - 1000 = 4000
    assert_eq!(member3_data.violation_lockout_until, 2); // locked until round 2

    // Check statistics
    let stats = client.get_statistics();
    assert_eq!(stats.total_rounds, 1);
    assert_eq!(stats.total_violations, 1);
}

#[test]
fn test_can_receive_conditions() {
    let env = Env::default();
    let token_address = Address::generate(&env);

    let config = RoscaConfig {
        contribution_amount: 1_000_000_000,
        contribution_period: 604_800,
        min_deposit: 3_000_000_000,
        recommended_deposit: 5_000_000_000,
        max_deposit: Some(10_000_000_000),
        insurance_rate: 2,
        max_insurance_pool: 20_000_000_000,
        max_insurance_coverage: 2_000_000_000,
        observation_contributions: 3,
        all_members_observation: true,
        cooldown_type: CooldownType::DynamicMembers,
        violation_grace_period: 86_400,
        violation_penalties: soroban_sdk::vec![
            &env,
            ViolationPenalty {
                deposit_deduction: 1_000_000_000,
                points_deduction: 3,
                lockout_rounds: 2,
            },
        ],
        max_violations: 3,
        late_fee_rates: soroban_sdk::vec![&env, 5, 10, 20],
        max_late_count: 3,
        max_beneficiary_loss_rate: 10,
        allow_join: true,
        require_sponsor: false,
        status: RoscaStatus::Active,
        token_address: token_address.clone(),
    };

    // Test 1: observation period incomplete, cannot receive
    let member_observing = Member {
        address: Address::generate(&env),
        joined_at: 0,
        status: MemberStatus::Active,
        is_system_account: false,
        deposit: 5_000_000_000,
        total_contributed: 2_000_000_000,
        total_received: 0,
        contribution_count: 2,
        receive_count: 0,
        violation_count: 0,
        late_count: 0,
        on_time_streak: 0,
        observation_count: 2, // fewer than required 3
        last_contribution_round: 2,
        last_received_round: 0,
        cooldown_until_round: 0,
        violation_lockout_until: 0,
        sponsored_by: None,
    };
    assert!(!member_observing.can_receive(&env, 3, 10, &config));

    // Test 2: priority score is 0, cannot receive
    let member_no_priority = Member {
        address: Address::generate(&env),
        joined_at: 0,
        status: MemberStatus::Active,
        is_system_account: false,
        deposit: 5_000_000_000,
        total_contributed: 10_000_000_000,
        total_received: 0,
        contribution_count: 10,
        receive_count: 1, // receive_count * members_count = 10
        violation_count: 0,
        late_count: 0,
        on_time_streak: 0,
        observation_count: 3,
        last_contribution_round: 10,
        last_received_round: 5,
        cooldown_until_round: 0,
        violation_lockout_until: 0,
        sponsored_by: None,
    };
    assert!(!member_no_priority.can_receive(&env, 11, 10, &config));

    // Test 3: in cooldown period, cannot receive
    let member_cooldown = Member {
        address: Address::generate(&env),
        joined_at: 0,
        status: MemberStatus::Active,
        is_system_account: false,
        deposit: 5_000_000_000,
        total_contributed: 12_000_000_000,
        total_received: 10_000_000_000,
        contribution_count: 12,
        receive_count: 1,
        violation_count: 0,
        late_count: 0,
        on_time_streak: 0,
        observation_count: 3,
        last_contribution_round: 12,
        last_received_round: 5,
        cooldown_until_round: 20, // cooldown until round 20
        violation_lockout_until: 0,
        sponsored_by: None,
    };
    assert!(!member_cooldown.can_receive(&env, 15, 10, &config));

    // Test 4: all conditions met, can receive
    let member_eligible = Member {
        address: Address::generate(&env),
        joined_at: 0,
        status: MemberStatus::Active,
        is_system_account: false,
        deposit: 5_000_000_000,
        total_contributed: 12_000_000_000,
        total_received: 10_000_000_000,
        contribution_count: 12,
        receive_count: 1,
        violation_count: 0,
        late_count: 0,
        on_time_streak: 0,
        observation_count: 3,
        last_contribution_round: 12,
        last_received_round: 5,
        cooldown_until_round: 10,
        violation_lockout_until: 0,
        sponsored_by: None,
    };
    assert!(member_eligible.can_receive(&env, 15, 10, &config));
}

#[test]
fn test_weighted_random_selection() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env); // highest priority
    let member2 = Address::generate(&env); // medium priority
    let member3 = Address::generate(&env); // lowest priority

    // Register a mock token contract
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    // Mint tokens to members for deposit and contributions
    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);
    token_client.mint(&member3, &100_000_000_000);

    let config = RoscaConfig {
        contribution_amount: 1_000_000_000,
        contribution_period: 604_800,
        min_deposit: 3_000_000_000,
        recommended_deposit: 5_000_000_000,
        max_deposit: Some(10_000_000_000),
        insurance_rate: 2,
        max_insurance_pool: 20_000_000_000,
        max_insurance_coverage: 2_000_000_000,
        observation_contributions: 0, // skip observation period
        all_members_observation: false,
        cooldown_type: CooldownType::DynamicMembers,
        violation_grace_period: 86_400,
        violation_penalties: soroban_sdk::vec![
            &env,
            ViolationPenalty {
                deposit_deduction: 1_000_000_000,
                points_deduction: 3,
                lockout_rounds: 2,
            },
        ],
        max_violations: 3,
        late_fee_rates: soroban_sdk::vec![&env, 5, 10, 20],
        max_late_count: 3,
        max_beneficiary_loss_rate: 10,
        allow_join: true,
        require_sponsor: false,
        status: RoscaStatus::Active,
        token_address: token_address.clone(),
    };

    client.initialize(&config);

    // Three members join
    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);
    client.join(&member3, &5_000_000_000);

    // All three contribute for 5 rounds to build up priority scores
    for _ in 0..5 {
        client.contribute(&member1);
        client.contribute(&member2);
        client.contribute(&member3);

        // Advance time past round end + grace period
        env.ledger().with_mut(|li| {
            li.timestamp += 604_800 + 86_400 + 1; // contribution_period + grace_period
        });

        client.settle_round(&999u64); // anyone can call
        // Note: settle_round advances the round counter
    }

    // Query highest priority candidate (read-only, for reference)
    let _highest_priority_recipient = client.calculate_recipient();

    // Settle with a specific seed to verify weighted random selection works
    // (different seeds may select different recipients in practice)
    client.contribute(&member1);
    client.contribute(&member2);
    client.contribute(&member3);

    // Advance time past round end + grace period
    env.ledger().with_mut(|li| {
        li.timestamp += 604_800 + 86_400 + 1; // contribution_period + grace_period
    });

    // member1 has the highest priority score and highest selection probability
    client.settle_round(&42u64);
    let round = client.get_round(&5);

    // Selection is not deterministic, but this verifies the function runs without error
    assert!(round.recipient.is_some());
}

/// Helper to create a standard test config
fn test_config(env: &Env, token_address: &Address) -> RoscaConfig {
    RoscaConfig {
        contribution_amount: 1_000_000_000,
        contribution_period: 604_800,
        min_deposit: 3_000_000_000,
        recommended_deposit: 5_000_000_000,
        max_deposit: Some(10_000_000_000),
        insurance_rate: 2,
        max_insurance_pool: 20_000_000_000,
        max_insurance_coverage: 2_000_000_000,
        observation_contributions: 0,
        all_members_observation: false,
        cooldown_type: CooldownType::DynamicMembers,
        violation_grace_period: 86_400,
        violation_penalties: soroban_sdk::vec![
            env,
            ViolationPenalty {
                deposit_deduction: 1_000_000_000,
                points_deduction: 3,
                lockout_rounds: 2,
            },
        ],
        max_violations: 3,
        late_fee_rates: soroban_sdk::vec![env, 5, 10, 20],
        max_late_count: 3,
        max_beneficiary_loss_rate: 10,
        allow_join: true,
        require_sponsor: false,
        status: RoscaStatus::Active,
        token_address: token_address.clone(),
    }
}


#[test]
fn test_sponsor_flow() {
    let env = Env::default();
    env.mock_all_auths();

    // Contract 1: bootstrap member with require_sponsor = false
    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);
    let candidate = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&candidate, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&config);

    // member1 joins (no sponsor needed, require_sponsor = false)
    client.join(&member1, &5_000_000_000);

    // member1 sponsors candidate
    client.sponsor(&member1, &candidate);

    // candidate can join
    client.join(&candidate, &5_000_000_000);

    let candidate_data = client.get_member(&candidate);
    assert_eq!(candidate_data.status, MemberStatus::Active);
    // Audit trail: sponsored_by is stored in Member
    assert_eq!(candidate_data.sponsored_by, Some(member1.clone()));

    // member1 joined without sponsor
    let member1_data = client.get_member(&member1);
    assert_eq!(member1_data.sponsored_by, None);
}

#[test]
fn test_sponsor_required_without_sponsor_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let candidate = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&candidate, &50_000_000_000);

    let mut config = test_config(&env, &token_address);
    config.require_sponsor = true;
    client.initialize(&config);

    // Candidate tries to join without sponsor — should fail
    let result = client.try_join(&candidate, &5_000_000_000);
    assert!(result.is_err());
}

#[test]
fn test_top_up_deposit() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&config);

    client.join(&member1, &5_000_000_000);

    // Top up deposit by 2B
    client.top_up_deposit(&member1, &2_000_000_000);

    let member_data = client.get_member(&member1);
    assert_eq!(member_data.deposit, 7_000_000_000); // 5B + 2B

    // Top up beyond max_deposit should fail (max is 10B, current is 7B, adding 4B = 11B)
    let result = client.try_top_up_deposit(&member1, &4_000_000_000);
    assert!(result.is_err());
}

#[test]
fn test_request_exit_and_settle() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);
    let exiting_member = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&member2, &50_000_000_000);
    token_client.mint(&exiting_member, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);
    client.join(&exiting_member, &5_000_000_000);

    // exiting_member requests exit
    client.request_exit(&exiting_member);

    let exit_data = client.get_member(&exiting_member);
    assert_eq!(exit_data.status, MemberStatus::ExitPending);

    // Only member1 and member2 contribute (exiting_member is ExitPending, shouldn't be expected)
    client.contribute(&member1);
    client.contribute(&member2);

    // Advance time past round end + grace period
    env.ledger().with_mut(|li| {
        li.timestamp += 604_800 + 86_400 + 1; // contribution_period + grace_period
    });

    // Settle round — should process ExitPending member
    client.settle_round(&42u64);

    // exiting_member should be removed
    let result = client.try_get_member(&exiting_member);
    assert!(result.is_err()); // member no longer exists

    // Members list should only have 2 members
    let members = client.get_members();
    assert_eq!(members.len(), 2);

    // Stats should reflect the exit
    let stats = client.get_statistics();
    assert_eq!(stats.active_members, 2);
}

#[test]
fn test_time_based_cooldown() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);

    let mut config = test_config(&env, &token_address);
    // contribution_period = 604_800 (1 week)
    // TimeBased cooldown of 3 weeks = 3 * 604_800 = 1_814_400 seconds
    // Expected cooldown_rounds = 1_814_400 / 604_800 = 3
    config.cooldown_type = CooldownType::TimeBased(1_814_400);
    client.initialize(&config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    // Both contribute for multiple rounds
    for _ in 0..5 {
        client.contribute(&member1);
        client.contribute(&member2);

        // Advance time past round end + grace period
        env.ledger().with_mut(|li| {
            li.timestamp += 604_800 + 86_400 + 1; // contribution_period + grace_period
        });

        client.settle_round(&99u64);
    }

    // After receiving payout, check cooldown_until_round
    // The recipient should have cooldown of 3 rounds from the round they received
    let m1 = client.get_member(&member1);
    let m2 = client.get_member(&member2);

    // One of them should have received and have cooldown set
    if m1.receive_count > 0 {
        // cooldown should be receive_round + 3
        assert_eq!(m1.cooldown_until_round, m1.last_received_round + 3);
    }
    if m2.receive_count > 0 {
        assert_eq!(m2.cooldown_until_round, m2.last_received_round + 3);
    }
}

#[test]
fn test_update_config_proposal() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&config);
    client.join(&member1, &5_000_000_000);

    // Contribute once so member has voting weight
    client.contribute(&member1);

    // Create UpdateConfig proposal with new contribution_amount
    let mut new_config = test_config(&env, &token_address);
    new_config.contribution_amount = 2_000_000_000; // double the amount

    let proposal_id = client.propose(&member1, &ProposalType::UpdateConfig(new_config.clone()));

    // Vote for it
    client.vote(&member1, &proposal_id, &VoteChoice::For);

    // Advance past voting period (7 days) + cooldown (7 days)
    env.ledger().with_mut(|li| {
        li.timestamp += 14 * 24 * 3600 + 1;
    });

    // Execute proposal
    client.execute_proposal(&member1, &proposal_id);

    // Verify config was updated
    let updated_config = client.get_config();
    assert_eq!(updated_config.contribution_amount, 2_000_000_000);
    // Token address should remain unchanged
    assert_eq!(updated_config.token_address, token_address);
}

#[test]
fn test_contribute_late_duplicate_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&config);
    client.join(&member1, &5_000_000_000);

    // Contribute normally during the contribution period
    env.ledger().with_mut(|li| {
        li.timestamp += 100; // Within contribution period
    });
    client.contribute(&member1);

    // Now advance to the grace period
    env.ledger().with_mut(|li| {
        li.timestamp += 604_800; // Past contribution period end
    });

    // Try to contribute_late again — should fail with AlreadyContributed
    let result = client.try_contribute_late(&member1);
    assert!(result.is_err());
}

#[test]
fn test_contribute_late_exit_pending_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&member2, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&config);
    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    // member1 requests exit
    client.request_exit(&member1);

    // Advance to grace period
    env.ledger().with_mut(|li| {
        li.timestamp += 604_800 + 100; // Past contribution period end, within grace period
    });

    // ExitPending member tries contribute_late — should fail
    let result = client.try_contribute_late(&member1);
    assert!(result.is_err());
}

#[test]
fn test_kicked_member_deposit_confiscated() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);
    let violator = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&violator, &50_000_000_000);

    // Set max_violations = 2 for faster testing
    let mut config = test_config(&env, &token_address);
    config.max_violations = 2;
    client.initialize(&config);

    client.join(&member1, &5_000_000_000);
    client.join(&violator, &5_000_000_000);

    // Round 0: member1 contributes, violator doesn't
    env.ledger().with_mut(|li| {
        li.timestamp += 100;
    });
    client.contribute(&member1);

    // Settle round 0 — violator gets 1st violation
    env.ledger().with_mut(|li| {
        li.timestamp = 604_800 + 86_400 + 1; // Past contribution period + grace period
    });
    client.settle_round(&42u64);

    // Round 1: member1 contributes within round 1's period [604800, 1209600)
    env.ledger().with_mut(|li| {
        li.timestamp = 604_800 + 100; // Within round 1 period
    });
    client.contribute(&member1);

    // Settle round 1 — violator reaches max_violations, gets kicked
    env.ledger().with_mut(|li| {
        li.timestamp = 2 * 604_800 + 86_400 + 1; // Past round 1's grace period
    });
    client.settle_round(&99u64);

    // Verify violator is kicked with 0 deposit
    let violator_data = client.get_member(&violator);
    assert_eq!(violator_data.status, MemberStatus::Kicked);
    assert_eq!(violator_data.deposit, 0); // Deposit fully confiscated
}

#[test]
fn test_observing_member_stats_tracking() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &50_000_000_000);

    // all_members_observation = true, observation_contributions = 1
    let mut config = test_config(&env, &token_address);
    config.all_members_observation = true;
    config.observation_contributions = 1;
    client.initialize(&config);

    // Join — member starts as Observing
    client.join(&member1, &5_000_000_000);

    let stats_after_join = client.get_statistics();
    assert_eq!(stats_after_join.total_members, 1);
    assert_eq!(stats_after_join.active_members, 0); // Not Active yet — Observing

    // Contribute to pass observation
    env.ledger().with_mut(|li| {
        li.timestamp += 100;
    });
    client.contribute(&member1);

    // After observation completed, member becomes Active
    let member_data = client.get_member(&member1);
    assert_eq!(member_data.status, MemberStatus::Active);

    let stats_after_promote = client.get_statistics();
    assert_eq!(stats_after_promote.active_members, 1); // Now Active
}

#[test]
fn test_top_up_deposit_kicked_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);
    let violator = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&violator, &50_000_000_000);

    let mut config = test_config(&env, &token_address);
    config.max_violations = 1; // Kicked on first violation
    client.initialize(&config);

    client.join(&member1, &5_000_000_000);
    client.join(&violator, &5_000_000_000);

    // member1 contributes, violator doesn't
    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);

    // Settle — violator gets kicked (max_violations = 1)
    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 86_400 + 1; });
    client.settle_round(&42u64);

    let v = client.get_member(&violator);
    assert_eq!(v.status, MemberStatus::Kicked);

    // Kicked member tries to top up — should fail
    let result = client.try_top_up_deposit(&violator, &1_000_000_000);
    assert!(result.is_err());
}

#[test]
fn test_update_config_cannot_change_status() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&config);
    client.join(&member1, &5_000_000_000);

    // Contribute to get voting weight
    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);

    // Propose UpdateConfig that tries to set status = Dissolved
    let mut malicious_config = test_config(&env, &token_address);
    malicious_config.contribution_amount = 2_000_000_000;
    malicious_config.status = RoscaStatus::Dissolved; // Attempt to bypass governance

    let proposal_id = client.propose(&member1, &ProposalType::UpdateConfig(malicious_config));
    client.vote(&member1, &proposal_id, &VoteChoice::For);

    // Advance past voting + cooldown
    env.ledger().with_mut(|li| { li.timestamp += 14 * 24 * 3600 + 1; });
    client.execute_proposal(&member1, &proposal_id);

    // Verify: contribution_amount updated but status preserved as Active
    let updated = client.get_config();
    assert_eq!(updated.contribution_amount, 2_000_000_000);
    assert_eq!(updated.status, RoscaStatus::Active); // Status NOT changed to Dissolved
}

#[test]
fn test_validate_config_rejects_invalid() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let token_address = Address::generate(&env);

    // insurance_rate > 100 should fail
    let mut config = test_config(&env, &token_address);
    config.insurance_rate = 101;
    let result = client.try_initialize(&config);
    assert!(result.is_err());

    // max_beneficiary_loss_rate > 100 should fail
    let mut config2 = test_config(&env, &token_address);
    config2.max_beneficiary_loss_rate = 150;
    let result2 = client.try_initialize(&config2);
    assert!(result2.is_err());

    // max_deposit < min_deposit should fail
    let mut config3 = test_config(&env, &token_address);
    config3.max_deposit = Some(1_000_000_000); // Less than min_deposit (3B)
    let result3 = client.try_initialize(&config3);
    assert!(result3.is_err());

    // empty violation_penalties should fail
    let mut config4 = test_config(&env, &token_address);
    config4.violation_penalties = soroban_sdk::vec![&env];
    let result4 = client.try_initialize(&config4);
    assert!(result4.is_err());
}

#[test]
fn test_insurance_pool_not_double_counted() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&member2, &50_000_000_000);

    let config = test_config(&env, &token_address);
    // insurance_rate = 2%, contribution_amount = 1_000_000_000
    client.initialize(&config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    // Both contribute — each adds 20_000_000 (2% of 1B) to insurance pool
    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);
    client.contribute(&member2);

    // Insurance pool should be 40_000_000 (from contribute calls)
    let pool_before_settle = client.get_insurance_pool();
    assert_eq!(pool_before_settle, 40_000_000);

    // Settle round — should NOT add insurance_collected again
    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 86_400 + 1; });
    client.settle_round(&42u64);

    // Insurance pool should still be 40_000_000 (not 80_000_000 from double-counting)
    let pool_after_settle = client.get_insurance_pool();
    assert_eq!(pool_after_settle, 40_000_000);
}

#[test]
fn test_settle_round_blocked_during_grace_period() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&member2, &50_000_000_000);

    let config = test_config(&env, &token_address);
    // violation_grace_period = 86_400 (1 day)
    client.initialize(&config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    // member1 contributes on time
    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);

    // Advance to just past round_end but within grace period
    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 1; });

    // settle_round should FAIL — grace period hasn't ended
    let result = client.try_settle_round(&42u64);
    assert!(result.is_err());

    // member2 contributes late during grace period
    client.contribute_late(&member2);

    // Advance past grace period
    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 86_400 + 1; });

    // Now settle should succeed — and member2 should NOT be a violator
    client.settle_round(&42u64);

    let round = client.get_round(&0);
    assert_eq!(round.actual_contributors.len(), 2); // Both contributed
    assert_eq!(round.violators.len(), 0); // No violators
}

#[test]
fn test_exit_refund_capped_at_contract_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);
    let token_balance_client = soroban_sdk::token::Client::new(&env, &token_address);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    // Both contribute for several rounds, member1 receives payout each time
    for _ in 0..3 {
        client.contribute(&member1);
        client.contribute(&member2);

        env.ledger().with_mut(|li| {
            li.timestamp += 604_800 + 86_400 + 1;
        });

        // Use seed that selects member1 (or whoever is eligible)
        client.settle_round(&0u64);
    }

    // member2 has contributed 3 rounds (3B) but never received
    // net_balance = 3B - 0 = 3B, deposit = 5B, exit claim = 8B
    // Contract balance may be less than 8B due to payouts
    let contract_balance_before = token_balance_client.balance(&contract_id);

    // member2 requests exit
    client.request_exit(&member2);

    // Contribute round 4 with only member1, settle to process exit
    client.contribute(&member1);

    env.ledger().with_mut(|li| {
        li.timestamp += 604_800 + 86_400 + 1;
    });

    // This should NOT panic even if exit claim exceeds contract balance
    client.settle_round(&0u64);

    // member2 should be removed
    let result = client.try_get_member(&member2);
    assert!(result.is_err());

    // Contract balance should be >= 0 (no underflow)
    let contract_balance_after = token_balance_client.balance(&contract_id);
    assert!(contract_balance_after >= 0);
}
