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

    // Advance time past round end
    env.ledger().with_mut(|li| {
        li.timestamp += 604_800; // contribution_period
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

        // Advance time past round end
        env.ledger().with_mut(|li| {
            li.timestamp += 604_800; // contribution_period
        });

        client.settle_round(&999u64); // anyone can call
        // Note: settle_round advances the round counter

        // Advance time to next round start
        env.ledger().with_mut(|li| {
            li.timestamp += 1; // move to next round start
        });
    }

    // Query highest priority candidate (read-only, for reference)
    let _highest_priority_recipient = client.calculate_recipient();

    // Settle with a specific seed to verify weighted random selection works
    // (different seeds may select different recipients in practice)
    client.contribute(&member1);
    client.contribute(&member2);
    client.contribute(&member3);

    // Advance time past round end
    env.ledger().with_mut(|li| {
        li.timestamp += 604_800; // contribution_period
    });

    // member1 has the highest priority score and highest selection probability
    client.settle_round(&42u64);
    let round = client.get_round(&5);

    // Selection is not deterministic, but this verifies the function runs without error
    assert!(round.recipient.is_some());
}
