#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::{Address as _, Ledger}, token::StellarAssetClient, Address, Env};

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
        max_members: 20,
        allow_join: true,
        require_sponsor: false,
        status: RoscaStatus::Active,
        token_address: token_address.clone(),
    }
}

/// Helper: set up env, contract, token, admin
fn setup() -> (Env, RoscaV2ContractClient<'static>, Address, Address, StellarAssetClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = StellarAssetClient::new(&env, &token_address);
    (env, client, admin, token_address, token_client)
}

// =========================================================
// Basic tests
// =========================================================

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
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
        max_members: 20,
        allow_join: true,
        require_sponsor: false,
        status: RoscaStatus::Active,
        token_address: token_address.clone(),
    };

    client.initialize(&admin, &config);

    let retrieved_config = client.get_config();
    assert_eq!(retrieved_config.contribution_amount, 1_000_000_000);
    assert_eq!(retrieved_config.allow_join, true);
}

#[test]
fn test_join() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    token_client.mint(&member1, &10_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);

    let member_data = client.get_member(&member1);
    assert_eq!(member_data.deposit, 5_000_000_000);
    assert_eq!(member_data.status, MemberStatus::Active);
}

#[test]
fn test_priority_score() {
    let env = Env::default();
    let token_address = Address::generate(&env);

    let config = test_config(&env, &token_address);

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
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);
    let member3 = Address::generate(&env);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&member2, &50_000_000_000);
    token_client.mint(&member3, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    // Three members join
    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);
    client.join(&member3, &5_000_000_000);

    // member1 and member2 contribute, member3 violates
    client.contribute(&member1);
    client.contribute(&member2);

    // Advance time past round end + grace period
    env.ledger().with_mut(|li| {
        li.timestamp += 604_800 + 86_400 + 1;
    });

    // Settle round (permissionless)
    client.settle_round();

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

    let mut config = test_config(&env, &token_address);
    config.observation_contributions = 3;
    config.all_members_observation = true;

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
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);
    let member3 = Address::generate(&env);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);
    token_client.mint(&member3, &100_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);
    client.join(&member3, &5_000_000_000);

    // All three contribute for 5 rounds to build up priority scores
    for _ in 0..5 {
        client.contribute(&member1);
        client.contribute(&member2);
        client.contribute(&member3);

        env.ledger().with_mut(|li| {
            li.timestamp += 604_800 + 86_400 + 1;
        });

        client.settle_round();
    }

    // Settle with a specific seed to verify weighted random selection works
    client.contribute(&member1);
    client.contribute(&member2);
    client.contribute(&member3);

    env.ledger().with_mut(|li| {
        li.timestamp += 604_800 + 86_400 + 1;
    });

    client.settle_round();
    let round = client.get_round(&5);

    // Selection is not deterministic, but this verifies the function runs without error
    assert!(round.recipient.is_some());
}

#[test]
fn test_sponsor_flow() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let candidate = Address::generate(&env);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&candidate, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    // member1 joins (no sponsor needed)
    client.join(&member1, &5_000_000_000);

    // member1 sponsors candidate
    client.sponsor(&member1, &candidate);

    // candidate can join
    client.join(&candidate, &5_000_000_000);

    let candidate_data = client.get_member(&candidate);
    assert_eq!(candidate_data.status, MemberStatus::Active);
    assert_eq!(candidate_data.sponsored_by, Some(member1.clone()));

    let member1_data = client.get_member(&member1);
    assert_eq!(member1_data.sponsored_by, None);
}

#[test]
fn test_sponsor_required_without_sponsor_fails() {
    let (env, client, admin, token_address, token_client) = setup();
    let candidate = Address::generate(&env);
    token_client.mint(&candidate, &50_000_000_000);

    let mut config = test_config(&env, &token_address);
    config.require_sponsor = true;
    client.initialize(&admin, &config);

    // Candidate tries to join without sponsor — should fail
    let result = client.try_join(&candidate, &5_000_000_000);
    assert!(result.is_err());
}

#[test]
fn test_top_up_deposit() {
    let (env, client, admin, token_address, token_client) = setup();
    let member1 = Address::generate(&env);
    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);
    client.join(&member1, &5_000_000_000);

    // Top up deposit by 2B
    client.top_up_deposit(&member1, &2_000_000_000);
    let member_data = client.get_member(&member1);
    assert_eq!(member_data.deposit, 7_000_000_000);

    // Top up beyond max_deposit should fail (max 10B, current 7B, adding 4B = 11B)
    let result = client.try_top_up_deposit(&member1, &4_000_000_000);
    assert!(result.is_err());
}

#[test]
fn test_request_exit_and_settle() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);
    let exiting_member = Address::generate(&env);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&member2, &50_000_000_000);
    token_client.mint(&exiting_member, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);
    client.join(&exiting_member, &5_000_000_000);

    // exiting_member requests exit
    client.request_exit(&exiting_member);
    let exit_data = client.get_member(&exiting_member);
    assert_eq!(exit_data.status, MemberStatus::ExitPending);

    // Only member1 and member2 contribute
    client.contribute(&member1);
    client.contribute(&member2);

    env.ledger().with_mut(|li| {
        li.timestamp += 604_800 + 86_400 + 1;
    });

    client.settle_round();

    // exiting_member should be removed
    let result = client.try_get_member(&exiting_member);
    assert!(result.is_err());

    let members = client.get_members();
    assert_eq!(members.len(), 2);

    let stats = client.get_statistics();
    assert_eq!(stats.active_members, 2);
}

#[test]
fn test_time_based_cooldown() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);

    let mut config = test_config(&env, &token_address);
    config.cooldown_type = CooldownType::TimeBased(1_814_400); // 3 weeks
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    for _ in 0..5 {
        client.contribute(&member1);
        client.contribute(&member2);

        env.ledger().with_mut(|li| {
            li.timestamp += 604_800 + 86_400 + 1;
        });

        client.settle_round();
    }

    let m1 = client.get_member(&member1);
    let m2 = client.get_member(&member2);

    if m1.receive_count > 0 {
        assert_eq!(m1.cooldown_until_round, m1.last_received_round + 3);
    }
    if m2.receive_count > 0 {
        assert_eq!(m2.cooldown_until_round, m2.last_received_round + 3);
    }
}

#[test]
fn test_update_config_proposal() {
    let (env, client, admin, token_address, token_client) = setup();
    let member1 = Address::generate(&env);
    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);
    client.join(&member1, &5_000_000_000);

    // Contribute once so member has voting weight
    client.contribute(&member1);

    let mut new_config = test_config(&env, &token_address);
    new_config.contribution_amount = 2_000_000_000;

    let proposal_id = client.propose(&member1, &ProposalType::UpdateConfig(new_config.clone()));
    client.vote(&member1, &proposal_id, &VoteChoice::For);

    // Advance past voting period (7 days) + cooldown (7 days)
    env.ledger().with_mut(|li| {
        li.timestamp += 14 * 24 * 3600 + 1;
    });

    client.execute_proposal(&member1, &proposal_id);

    let updated_config = client.get_config();
    assert_eq!(updated_config.contribution_amount, 2_000_000_000);
    assert_eq!(updated_config.token_address, token_address);
}

#[test]
fn test_contribute_late_duplicate_rejected() {
    let (env, client, admin, token_address, token_client) = setup();
    let member1 = Address::generate(&env);
    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);
    client.join(&member1, &5_000_000_000);

    // Contribute normally
    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);

    // Advance to grace period
    env.ledger().with_mut(|li| { li.timestamp += 604_800; });

    // Try to contribute_late again — should fail
    let result = client.try_contribute_late(&member1);
    assert!(result.is_err());
}

#[test]
fn test_contribute_late_exit_pending_rejected() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&member2, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);
    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    client.request_exit(&member1);

    // Advance to grace period
    env.ledger().with_mut(|li| { li.timestamp += 604_800 + 100; });

    let result = client.try_contribute_late(&member1);
    assert!(result.is_err());
}

#[test]
fn test_kicked_member_deposit_confiscated() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let violator = Address::generate(&env);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&violator, &50_000_000_000);

    let mut config = test_config(&env, &token_address);
    config.max_violations = 2;
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&violator, &5_000_000_000);

    // Round 0: member1 contributes, violator doesn't
    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);

    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 86_400 + 1; });
    client.settle_round();

    // Round 1
    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 100; });
    client.contribute(&member1);

    env.ledger().with_mut(|li| { li.timestamp = 2 * 604_800 + 86_400 + 1; });
    client.settle_round();

    // Verify violator is removed
    let result = client.try_get_member(&violator);
    assert!(result.is_err());

    let members = client.get_members();
    assert_eq!(members.len(), 1);
}

#[test]
fn test_observing_member_stats_tracking() {
    let (env, client, admin, token_address, token_client) = setup();
    let member1 = Address::generate(&env);
    token_client.mint(&member1, &50_000_000_000);

    let mut config = test_config(&env, &token_address);
    config.all_members_observation = true;
    config.observation_contributions = 1;
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);

    let stats_after_join = client.get_statistics();
    assert_eq!(stats_after_join.total_members, 1);
    assert_eq!(stats_after_join.active_members, 0);

    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);

    let member_data = client.get_member(&member1);
    assert_eq!(member_data.status, MemberStatus::Active);

    let stats_after_promote = client.get_statistics();
    assert_eq!(stats_after_promote.active_members, 1);
}

#[test]
fn test_top_up_deposit_kicked_rejected() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let violator = Address::generate(&env);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&violator, &50_000_000_000);

    let mut config = test_config(&env, &token_address);
    config.max_violations = 1;
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&violator, &5_000_000_000);

    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);

    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 86_400 + 1; });
    client.settle_round();

    let v_result = client.try_get_member(&violator);
    assert!(v_result.is_err());

    let result = client.try_top_up_deposit(&violator, &1_000_000_000);
    assert!(result.is_err());
}

#[test]
fn test_update_config_cannot_change_status() {
    let (env, client, admin, token_address, token_client) = setup();
    let member1 = Address::generate(&env);
    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);
    client.join(&member1, &5_000_000_000);

    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);

    let mut malicious_config = test_config(&env, &token_address);
    malicious_config.contribution_amount = 2_000_000_000;
    malicious_config.status = RoscaStatus::Dissolved;

    let proposal_id = client.propose(&member1, &ProposalType::UpdateConfig(malicious_config));
    client.vote(&member1, &proposal_id, &VoteChoice::For);

    env.ledger().with_mut(|li| { li.timestamp += 14 * 24 * 3600 + 1; });
    client.execute_proposal(&member1, &proposal_id);

    let updated = client.get_config();
    assert_eq!(updated.contribution_amount, 2_000_000_000);
    assert_eq!(updated.status, RoscaStatus::Active);
}

#[test]
fn test_validate_config_rejects_invalid() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token_address = Address::generate(&env);

    // M2: insurance_rate >= 50 should fail
    let mut config = test_config(&env, &token_address);
    config.insurance_rate = 50;
    let result = client.try_initialize(&admin, &config);
    assert!(result.is_err());

    // insurance_rate = 101 should fail
    let mut config1b = test_config(&env, &token_address);
    config1b.insurance_rate = 101;
    let result1b = client.try_initialize(&admin, &config1b);
    assert!(result1b.is_err());

    // max_beneficiary_loss_rate > 100 should fail
    let mut config2 = test_config(&env, &token_address);
    config2.max_beneficiary_loss_rate = 150;
    let result2 = client.try_initialize(&admin, &config2);
    assert!(result2.is_err());

    // max_deposit < min_deposit should fail
    let mut config3 = test_config(&env, &token_address);
    config3.max_deposit = Some(1_000_000_000);
    let result3 = client.try_initialize(&admin, &config3);
    assert!(result3.is_err());

    // empty violation_penalties should fail
    let mut config4 = test_config(&env, &token_address);
    config4.violation_penalties = soroban_sdk::vec![&env];
    let result4 = client.try_initialize(&admin, &config4);
    assert!(result4.is_err());

    // max_violations = 0 should fail
    let mut config5 = test_config(&env, &token_address);
    config5.max_violations = 0;
    let result5 = client.try_initialize(&admin, &config5);
    assert!(result5.is_err());

    // late_fee_rates empty when max_late_count > 0 should fail
    let mut config6 = test_config(&env, &token_address);
    config6.late_fee_rates = soroban_sdk::vec![&env];
    config6.max_late_count = 1;
    let result6 = client.try_initialize(&admin, &config6);
    assert!(result6.is_err());

    // recommended_deposit > max_deposit should fail
    let mut config7 = test_config(&env, &token_address);
    config7.recommended_deposit = 12_000_000_000;
    config7.max_deposit = Some(10_000_000_000);
    let result7 = client.try_initialize(&admin, &config7);
    assert!(result7.is_err());
}

#[test]
fn test_insurance_pool_not_double_counted() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&member2, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    // Both contribute — each adds 20_000_000 (2% of 1B) to insurance pool
    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);
    client.contribute(&member2);

    let pool_before_settle = client.get_insurance_pool();
    assert_eq!(pool_before_settle, 40_000_000);

    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 86_400 + 1; });
    client.settle_round();

    // Insurance pool should still be 40_000_000 (not double-counted)
    let pool_after_settle = client.get_insurance_pool();
    assert_eq!(pool_after_settle, 40_000_000);
}

#[test]
fn test_settle_round_blocked_during_grace_period() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&member2, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);

    // Just past round_end but within grace period
    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 1; });

    // settle_round should FAIL
    let result = client.try_settle_round();
    assert!(result.is_err());

    // member2 contributes late during grace period
    client.contribute_late(&member2);

    // Advance past grace period
    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 86_400 + 1; });

    client.settle_round();

    let round = client.get_round(&0);
    assert_eq!(round.actual_contributors.len(), 2);
    assert_eq!(round.violators.len(), 0);
}

// =========================================================
// H2: Comprehensive governance tests
// =========================================================

#[test]
fn test_emergency_payout_governance() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);
    let member3 = Address::generate(&env);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);
    token_client.mint(&member3, &100_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);
    client.join(&member3, &5_000_000_000);

    // All contribute for a few rounds so they have voting weight and net balance
    for _ in 0..3 {
        client.contribute(&member1);
        client.contribute(&member2);
        client.contribute(&member3);

        env.ledger().with_mut(|li| {
            li.timestamp += 604_800 + 86_400 + 1;
        });
        client.settle_round();
    }

    // member1 proposes emergency payout for themselves
    let m1_data = client.get_member(&member1);
    let payout_amount = m1_data.net_balance().min(500_000_000); // small amount

    let details = EmergencyPayoutDetails {
        requester: member1.clone(),
        amount: payout_amount,
    };
    let proposal_id = client.propose(&member1, &ProposalType::EmergencyPayout(details));

    // All 3 members vote (need >66%)
    client.vote(&member1, &proposal_id, &VoteChoice::For);
    client.vote(&member2, &proposal_id, &VoteChoice::For);
    client.vote(&member3, &proposal_id, &VoteChoice::For);

    // Advance past voting period (48 hours)
    env.ledger().with_mut(|li| {
        li.timestamp += 48 * 3600 + 1;
    });

    // Execute
    client.execute_proposal(&member1, &proposal_id);

    // Verify proposal is executed
    let proposal = client.get_proposal(&proposal_id);
    assert!(proposal.executed);

    // Verify member1's receive_count increased
    let m1_after = client.get_member(&member1);
    assert!(m1_after.total_received > m1_data.total_received);
}

#[test]
fn test_dissolution_governance() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    // Contribute to get voting weight
    client.contribute(&member1);
    client.contribute(&member2);

    // Propose emergency dissolution (>75% threshold, 24h voting)
    let proposal_id = client.propose(&member1, &ProposalType::Dissolution(DissolutionMode::Emergency));

    // Both vote for (100% > 75%)
    client.vote(&member1, &proposal_id, &VoteChoice::For);
    client.vote(&member2, &proposal_id, &VoteChoice::For);

    // Advance past voting period (24 hours)
    env.ledger().with_mut(|li| {
        li.timestamp += 24 * 3600 + 1;
    });

    // Execute dissolution
    client.execute_proposal(&member1, &proposal_id);

    // Verify ROSCA is dissolved
    let config_after = client.get_config();
    assert_eq!(config_after.status, RoscaStatus::Dissolved);

    // Verify proposal is executed
    let proposal = client.get_proposal(&proposal_id);
    assert!(proposal.executed);
}

#[test]
fn test_double_vote_rejected() {
    let (env, client, admin, token_address, token_client) = setup();
    let member1 = Address::generate(&env);
    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);
    client.join(&member1, &5_000_000_000);

    client.contribute(&member1);

    let mut new_config = test_config(&env, &token_address);
    new_config.contribution_amount = 2_000_000_000;
    let proposal_id = client.propose(&member1, &ProposalType::UpdateConfig(new_config));

    client.vote(&member1, &proposal_id, &VoteChoice::For);

    // Second vote should fail
    let result = client.try_vote(&member1, &proposal_id, &VoteChoice::For);
    assert!(result.is_err());
}

#[test]
fn test_expired_voting_rejected() {
    let (env, client, admin, token_address, token_client) = setup();
    let member1 = Address::generate(&env);
    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);
    client.join(&member1, &5_000_000_000);

    client.contribute(&member1);

    let mut new_config = test_config(&env, &token_address);
    new_config.contribution_amount = 2_000_000_000;
    let proposal_id = client.propose(&member1, &ProposalType::UpdateConfig(new_config));

    // Advance past voting period (7 days) — don't vote yet
    env.ledger().with_mut(|li| {
        li.timestamp += 7 * 24 * 3600 + 1;
    });

    // Voting should fail — period ended
    let result = client.try_vote(&member1, &proposal_id, &VoteChoice::For);
    assert!(result.is_err());
}

#[test]
fn test_insufficient_votes_rejected() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);
    let member3 = Address::generate(&env);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);
    token_client.mint(&member3, &100_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);
    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);
    client.join(&member3, &5_000_000_000);

    // All contribute equally (each gets weight 1)
    client.contribute(&member1);
    client.contribute(&member2);
    client.contribute(&member3);

    // Propose emergency dissolution (needs >75%)
    let proposal_id = client.propose(&member1, &ProposalType::Dissolution(DissolutionMode::Emergency));

    // Only 1 out of 3 votes for — 33% < 75%
    client.vote(&member1, &proposal_id, &VoteChoice::For);

    env.ledger().with_mut(|li| {
        li.timestamp += 24 * 3600 + 1;
    });

    // Execution should fail
    let result = client.try_execute_proposal(&member1, &proposal_id);
    assert!(result.is_err());
}

#[test]
fn test_execute_before_voting_ends_rejected() {
    let (env, client, admin, token_address, token_client) = setup();
    let member1 = Address::generate(&env);
    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);
    client.join(&member1, &5_000_000_000);

    client.contribute(&member1);

    let mut new_config = test_config(&env, &token_address);
    new_config.contribution_amount = 2_000_000_000;
    let proposal_id = client.propose(&member1, &ProposalType::UpdateConfig(new_config));
    client.vote(&member1, &proposal_id, &VoteChoice::For);

    // Try to execute immediately — voting hasn't ended
    let result = client.try_execute_proposal(&member1, &proposal_id);
    assert!(result.is_err());
}

// =========================================================
// Full lifecycle test
// =========================================================

#[test]
fn test_full_lifecycle() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);
    let member3 = Address::generate(&env);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);
    token_client.mint(&member3, &100_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    // All join
    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);
    client.join(&member3, &5_000_000_000);

    let stats = client.get_statistics();
    assert_eq!(stats.total_members, 3);
    assert_eq!(stats.active_members, 3);

    // Run 6 rounds — each member should eventually receive at least once
    let mut any_received = false;
    for round_num in 0u64..6 {
        client.contribute(&member1);
        client.contribute(&member2);
        client.contribute(&member3);

        env.ledger().with_mut(|li| {
            li.timestamp += 604_800 + 86_400 + 1;
        });

        client.settle_round();

        let round = client.get_round(&round_num);
        if round.recipient.is_some() {
            any_received = true;
        }
    }

    assert!(any_received);

    // Verify stats
    let final_stats = client.get_statistics();
    assert_eq!(final_stats.total_rounds, 6);
    assert_eq!(final_stats.total_violations, 0);

    // Members can exit
    let m1 = client.get_member(&member1);
    if m1.can_exit() {
        client.request_exit(&member1);
    }
}

// =========================================================
// Violation test with three-layer compensation
// =========================================================

#[test]
fn test_violation_three_layer_compensation() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env); // will receive
    let member2 = Address::generate(&env); // will contribute
    let violator = Address::generate(&env); // will violate

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);
    token_client.mint(&violator, &100_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);
    client.join(&violator, &5_000_000_000);

    // Run a few rounds with everyone contributing
    for _ in 0..3 {
        client.contribute(&member1);
        client.contribute(&member2);
        client.contribute(&violator);

        env.ledger().with_mut(|li| {
            li.timestamp += 604_800 + 86_400 + 1;
        });
        client.settle_round();
    }

    // Now violator doesn't contribute
    client.contribute(&member1);
    client.contribute(&member2);

    env.ledger().with_mut(|li| {
        li.timestamp += 604_800 + 86_400 + 1;
    });

    client.settle_round();

    let current_round_id = client.get_current_round() - 1;
    let round = client.get_round(&current_round_id);

    // Verify violation was recorded
    assert_eq!(round.violators.len(), 1);

    // Verify deposit compensation was applied
    assert!(round.deposit_compensation > 0);

    // Verify the violator lost deposit
    let v_data = client.get_member(&violator);
    assert!(v_data.deposit < 5_000_000_000);
    assert_eq!(v_data.violation_count, 1);
}

// =========================================================
// Late contribution happy path
// =========================================================

#[test]
fn test_late_contribution_happy_path() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    // member1 contributes on time
    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);

    // member2 misses deadline, contributes during grace period
    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 100; });
    client.contribute_late(&member2);

    let m2 = client.get_member(&member2);
    assert_eq!(m2.late_count, 1);
    assert_eq!(m2.contribution_count, 1);
    assert_eq!(m2.on_time_streak, 0); // Reset for late

    // Settle round — member2 should NOT be a violator
    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 86_400 + 1; });
    client.settle_round();

    let round = client.get_round(&0);
    assert_eq!(round.actual_contributors.len(), 2);
    assert_eq!(round.violators.len(), 0);
}

// =========================================================
// No-recipient refund path
// =========================================================

#[test]
fn test_no_recipient_refund_path() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);

    // All members need observation, contributions = 3
    let mut config = test_config(&env, &token_address);
    config.all_members_observation = true;
    config.observation_contributions = 3;
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    // Both contribute round 0 — still Observing (observation_count will be 1 after this)
    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);
    client.contribute(&member2);

    // Settle round 0 — no eligible recipient (all observing, no one is Active yet)
    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 86_400 + 1; });
    client.settle_round();

    let round = client.get_round(&0);
    assert!(round.recipient.is_none());
    assert_eq!(round.payout_amount, 0);

    // Verify members got refunded (contribution minus insurance, each member)
    // They each contributed 1B, insurance = 2% = 20M, so refund = 980M each
    // (Not easily verifiable without token balance check, but the round succeeded without panic)
}

// =========================================================
// C3: Permissionless settle_round
// =========================================================

#[test]
fn test_permissionless_settle_round() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);
    let random_caller = Address::generate(&env); // Not admin, not a member

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&member2, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    client.contribute(&member1);
    client.contribute(&member2);

    env.ledger().with_mut(|li| {
        li.timestamp += 604_800 + 86_400 + 1;
    });

    // Anyone can settle — no admin auth needed
    // The contract doesn't require_auth on settle_round anymore
    client.settle_round();

    let round = client.get_round(&0);
    assert_eq!(round.actual_contributors.len(), 2);
    assert_eq!(round.round_id, 0);
}

// =========================================================
// H1: Insurance pool cap test
// =========================================================

#[test]
fn test_insurance_pool_cap_no_orphaned_tokens() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);

    // Set a very small max_insurance_pool so it fills up fast
    let mut config = test_config(&env, &token_address);
    config.max_insurance_pool = 10_000_000; // Very small: 10M
    config.insurance_rate = 2; // 2% of 1B = 20M per contribution (more than max pool)
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    // member1 contributes — insurance would be 20M but pool max is 10M
    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);

    let pool_after_m1 = client.get_insurance_pool();
    assert_eq!(pool_after_m1, 10_000_000); // Capped at max

    // member2 contributes — pool already full, no more insurance should be added
    client.contribute(&member2);

    let pool_after_m2 = client.get_insurance_pool();
    assert_eq!(pool_after_m2, 10_000_000); // Still capped

    // Settle round
    env.ledger().with_mut(|li| { li.timestamp = 604_800 + 86_400 + 1; });
    client.settle_round();

    // The round's actual_insurance should reflect what was actually added (10M, not 40M)
    let round = client.get_round(&0);
    assert_eq!(round.actual_insurance, 10_000_000);

    // Key: payout should be total_collected - actual_insurance = 2B - 10M = 1_990_000_000
    // NOT total_collected - theoretical_insurance (2B - 40M = 1_960_000_000)
    // The extra 10M that didn't go to insurance stays in the payout pool
    if round.recipient.is_some() {
        // Payout should be close to pool_amount which uses actual insurance
        assert!(round.payout_amount > 0);
    }
}

// =========================================================
// M5: Sponsor overwrite rejection
// =========================================================

#[test]
fn test_sponsor_overwrite_rejected() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);
    let candidate = Address::generate(&env);

    token_client.mint(&member1, &50_000_000_000);
    token_client.mint(&member2, &50_000_000_000);
    token_client.mint(&candidate, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    // member1 sponsors candidate
    client.sponsor(&member1, &candidate);

    // member2 tries to sponsor same candidate — should fail
    let result = client.try_sponsor(&member2, &candidate);
    assert!(result.is_err());
}

// =========================================================
// Double init should fail
// =========================================================

#[test]
fn test_double_init_rejected() {
    let (env, client, admin, token_address, _token_client) = setup();
    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    // Second init should fail
    let result = client.try_initialize(&admin, &config);
    assert!(result.is_err());
}

// =========================================================
// Double join should fail
// =========================================================

#[test]
fn test_double_join_rejected() {
    let (env, client, admin, token_address, token_client) = setup();
    let member1 = Address::generate(&env);
    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);
    client.join(&member1, &5_000_000_000);

    // Second join should fail
    let result = client.try_join(&member1, &5_000_000_000);
    assert!(result.is_err());
}

// =========================================================
// Double contribute should fail
// =========================================================

#[test]
fn test_double_contribute_rejected() {
    let (env, client, admin, token_address, token_client) = setup();
    let member1 = Address::generate(&env);
    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);
    client.join(&member1, &5_000_000_000);

    env.ledger().with_mut(|li| { li.timestamp += 100; });
    client.contribute(&member1);

    // Second contribute should fail
    let result = client.try_contribute(&member1);
    assert!(result.is_err());
}

// =========================================================
// ExceedsMaxDeposit error test
// =========================================================

#[test]
fn test_exceeds_max_deposit_on_join() {
    let (env, client, admin, token_address, token_client) = setup();
    let member1 = Address::generate(&env);
    token_client.mint(&member1, &50_000_000_000);

    let config = test_config(&env, &token_address);
    // max_deposit = 10B
    client.initialize(&admin, &config);

    // Try to join with deposit > max_deposit
    let result = client.try_join(&member1, &15_000_000_000);
    assert!(result.is_err());
}

// =========================================================
// Normal dissolution with >90% threshold
// =========================================================

#[test]
fn test_normal_dissolution() {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);

    // Contribute to get voting weight
    client.contribute(&member1);
    client.contribute(&member2);

    // Propose normal dissolution (>90% threshold, 14d voting)
    let proposal_id = client.propose(&member1, &ProposalType::Dissolution(DissolutionMode::Normal));

    // Both vote (100% > 90%)
    client.vote(&member1, &proposal_id, &VoteChoice::For);
    client.vote(&member2, &proposal_id, &VoteChoice::For);

    // Advance past 14-day voting period
    env.ledger().with_mut(|li| {
        li.timestamp += 14 * 24 * 3600 + 1;
    });

    // Execute
    client.execute_proposal(&member1, &proposal_id);

    let config_after = client.get_config();
    assert_eq!(config_after.status, RoscaStatus::Dissolved);
}

// =========================================================
// Pause / Resume tests
// =========================================================

/// Helper: set up a ROSCA with members who have voting weight, returns
/// (env, client, admin, token_address, token_client, member1, member2, member3)
fn setup_with_voting_members() -> (
    Env,
    RoscaV2ContractClient<'static>,
    Address,
    Address,
    StellarAssetClient<'static>,
    Address,
    Address,
    Address,
) {
    let (env, client, admin, token_address, token_client) = setup();

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);
    let member3 = Address::generate(&env);

    token_client.mint(&member1, &100_000_000_000);
    token_client.mint(&member2, &100_000_000_000);
    token_client.mint(&member3, &100_000_000_000);

    let config = test_config(&env, &token_address);
    client.initialize(&admin, &config);

    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);
    client.join(&member3, &5_000_000_000);

    // All contribute for 2 rounds to build voting weight
    for _ in 0..2 {
        client.contribute(&member1);
        client.contribute(&member2);
        client.contribute(&member3);

        env.ledger().with_mut(|li| {
            li.timestamp += 604_800 + 86_400 + 1;
        });
        client.settle_round();
    }

    (env, client, admin, token_address, token_client, member1, member2, member3)
}

/// Helper: propose and execute a Pause proposal
fn do_pause(
    env: &Env,
    client: &RoscaV2ContractClient,
    member1: &Address,
    member2: &Address,
    member3: &Address,
) {
    let proposal_id = client.propose(member1, &ProposalType::Pause);
    client.vote(member1, &proposal_id, &VoteChoice::For);
    client.vote(member2, &proposal_id, &VoteChoice::For);
    client.vote(member3, &proposal_id, &VoteChoice::For);

    env.ledger().with_mut(|li| {
        li.timestamp += 48 * 3600 + 1;
    });

    client.execute_proposal(member1, &proposal_id);
}

#[test]
fn test_pause_proposal_and_resume() {
    let (env, client, _admin, _token_address, _token_client, member1, member2, member3) =
        setup_with_voting_members();

    // Propose pause
    let pause_id = client.propose(&member1, &ProposalType::Pause);

    // All vote for (100% > 66%)
    client.vote(&member1, &pause_id, &VoteChoice::For);
    client.vote(&member2, &pause_id, &VoteChoice::For);
    client.vote(&member3, &pause_id, &VoteChoice::For);

    // Advance past 48h voting period
    env.ledger().with_mut(|li| {
        li.timestamp += 48 * 3600 + 1;
    });

    // Execute pause
    client.execute_proposal(&member1, &pause_id);

    // Verify paused
    let config = client.get_config();
    assert_eq!(config.status, RoscaStatus::Paused);

    // Verify proposal executed
    let proposal = client.get_proposal(&pause_id);
    assert!(proposal.executed);

    // Now propose resume
    let resume_id = client.propose(&member1, &ProposalType::Resume);

    // >50% votes for — 2 out of 3 is enough
    client.vote(&member1, &resume_id, &VoteChoice::For);
    client.vote(&member2, &resume_id, &VoteChoice::For);

    // Advance past 48h voting period
    env.ledger().with_mut(|li| {
        li.timestamp += 48 * 3600 + 1;
    });

    // Execute resume
    client.execute_proposal(&member1, &resume_id);

    // Verify active again
    let config_after = client.get_config();
    assert_eq!(config_after.status, RoscaStatus::Active);
}

#[test]
fn test_pause_blocks_contributions() {
    let (env, client, _admin, _token_address, _token_client, member1, member2, member3) =
        setup_with_voting_members();

    do_pause(&env, &client, &member1, &member2, &member3);

    // contribute should fail with RoscaPaused
    let result = client.try_contribute(&member1);
    assert_eq!(
        result,
        Err(Ok(Error::RoscaPaused))
    );

    // contribute_late should fail with RoscaPaused
    let result = client.try_contribute_late(&member1);
    assert_eq!(
        result,
        Err(Ok(Error::RoscaPaused))
    );

    // settle_round should fail with RoscaPaused
    let result = client.try_settle_round();
    assert_eq!(
        result,
        Err(Ok(Error::RoscaPaused))
    );

    // join should fail with RoscaPaused
    let new_member = Address::generate(&env);
    let result = client.try_join(&new_member, &5_000_000_000);
    assert_eq!(
        result,
        Err(Ok(Error::RoscaPaused))
    );

    // sponsor should fail with RoscaPaused
    let candidate = Address::generate(&env);
    let result = client.try_sponsor(&member1, &candidate);
    assert_eq!(
        result,
        Err(Ok(Error::RoscaPaused))
    );
}

#[test]
fn test_pause_allows_exit() {
    let (env, client, _admin, _token_address, _token_client, member1, member2, member3) =
        setup_with_voting_members();

    do_pause(&env, &client, &member1, &member2, &member3);

    // Find a member who can exit (net_balance >= 0, i.e., hasn't received more than contributed)
    // Try each member — at least one should be eligible
    let members = [&member1, &member2, &member3];
    let mut exited = false;
    for m in members {
        let data = client.get_member(m);
        if data.can_exit() {
            // request_exit should work during pause
            client.request_exit(m);
            let after = client.get_member(m);
            assert_eq!(after.status, MemberStatus::ExitPending);
            exited = true;
            break;
        }
    }
    assert!(exited, "At least one member should be able to exit");
}

#[test]
fn test_pause_time_adjustment() {
    let (env, client, _admin, _token_address, _token_client, member1, member2, member3) =
        setup_with_voting_members();

    // Record current round and start time before pause
    let current_round = client.get_current_round();

    // Pause
    do_pause(&env, &client, &member1, &member2, &member3);

    // Wait 10 days during pause
    let pause_wait = 10 * 24 * 3600;
    env.ledger().with_mut(|li| {
        li.timestamp += pause_wait;
    });

    // Resume
    let resume_id = client.propose(&member1, &ProposalType::Resume);
    client.vote(&member1, &resume_id, &VoteChoice::For);
    client.vote(&member2, &resume_id, &VoteChoice::For);

    env.ledger().with_mut(|li| {
        li.timestamp += 48 * 3600 + 1;
    });

    client.execute_proposal(&member1, &resume_id);

    // Verify round number hasn't changed
    assert_eq!(client.get_current_round(), current_round);

    // After resume, members should be able to contribute in the current round
    // The contribution period should be fresh from the resume point
    client.contribute(&member1);
    client.contribute(&member2);

    // Advance past the contribution period + grace period to settle
    env.ledger().with_mut(|li| {
        li.timestamp += 604_800 + 86_400 + 1;
    });

    // settle_round should work
    client.settle_round();

    // Round should advance
    assert_eq!(client.get_current_round(), current_round + 1);
}

#[test]
fn test_cannot_pause_when_already_paused() {
    let (env, client, _admin, _token_address, _token_client, member1, member2, member3) =
        setup_with_voting_members();

    do_pause(&env, &client, &member1, &member2, &member3);

    // Trying to propose another pause should fail
    let result = client.try_propose(&member1, &ProposalType::Pause);
    assert_eq!(
        result,
        Err(Ok(Error::RoscaPaused))
    );
}

#[test]
fn test_cannot_resume_when_not_paused() {
    let (_env, client, _admin, _token_address, _token_client, member1, _member2, _member3) =
        setup_with_voting_members();

    // Trying to propose resume when Active should fail
    let result = client.try_propose(&member1, &ProposalType::Resume);
    assert_eq!(
        result,
        Err(Ok(Error::NotPaused))
    );
}

#[test]
fn test_dissolution_during_pause() {
    let (env, client, _admin, _token_address, _token_client, member1, member2, member3) =
        setup_with_voting_members();

    do_pause(&env, &client, &member1, &member2, &member3);

    // Propose emergency dissolution while paused
    let proposal_id = client.propose(&member1, &ProposalType::Dissolution(DissolutionMode::Emergency));

    // All vote for (100% > 75%)
    client.vote(&member1, &proposal_id, &VoteChoice::For);
    client.vote(&member2, &proposal_id, &VoteChoice::For);
    client.vote(&member3, &proposal_id, &VoteChoice::For);

    // Advance past 24h voting period
    env.ledger().with_mut(|li| {
        li.timestamp += 24 * 3600 + 1;
    });

    // Execute dissolution
    client.execute_proposal(&member1, &proposal_id);

    // Verify dissolved
    let config = client.get_config();
    assert_eq!(config.status, RoscaStatus::Dissolved);
}

#[test]
fn test_contribute_after_resume() {
    let (env, client, _admin, _token_address, _token_client, member1, member2, member3) =
        setup_with_voting_members();

    let round_before = client.get_current_round();

    // Pause
    do_pause(&env, &client, &member1, &member2, &member3);

    // Wait 5 days during pause
    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 3600;
    });

    // Resume
    let resume_id = client.propose(&member1, &ProposalType::Resume);
    client.vote(&member1, &resume_id, &VoteChoice::For);
    client.vote(&member2, &resume_id, &VoteChoice::For);
    client.vote(&member3, &resume_id, &VoteChoice::For);

    env.ledger().with_mut(|li| {
        li.timestamp += 48 * 3600 + 1;
    });

    client.execute_proposal(&member1, &resume_id);

    // All members contribute
    client.contribute(&member1);
    client.contribute(&member2);
    client.contribute(&member3);

    // Advance past round + grace
    env.ledger().with_mut(|li| {
        li.timestamp += 604_800 + 86_400 + 1;
    });

    // Settle should succeed
    client.settle_round();

    // Verify round advanced
    assert_eq!(client.get_current_round(), round_before + 1);

    // Verify contributions were recorded
    let round = client.get_round(&round_before);
    assert_eq!(round.actual_contributors.len(), 3);
    assert_eq!(round.violators.len(), 0);
}

// =========================================================
// max_members tests
// =========================================================

#[test]
fn test_max_members_enforced() {
    let (env, client, admin, token_address, token_client) = setup();

    let mut config = test_config(&env, &token_address);
    config.max_members = 3;
    client.initialize(&admin, &config);

    let member1 = Address::generate(&env);
    let member2 = Address::generate(&env);
    let member3 = Address::generate(&env);
    let member4 = Address::generate(&env);

    token_client.mint(&member1, &10_000_000_000);
    token_client.mint(&member2, &10_000_000_000);
    token_client.mint(&member3, &10_000_000_000);
    token_client.mint(&member4, &10_000_000_000);

    // First 3 members should join successfully
    client.join(&member1, &5_000_000_000);
    client.join(&member2, &5_000_000_000);
    client.join(&member3, &5_000_000_000);

    // 4th member should fail with GroupFull
    let result = client.try_join(&member4, &5_000_000_000);
    assert!(result.is_err());
}

#[test]
fn test_max_members_validation() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RoscaV2Contract);
    let client = RoscaV2ContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token_address = Address::generate(&env);

    // max_members = 0 should fail
    let mut config0 = test_config(&env, &token_address);
    config0.max_members = 0;
    let result0 = client.try_initialize(&admin, &config0);
    assert!(result0.is_err());

    // max_members = 1 should fail (ROSCA needs at least 2)
    let mut config1 = test_config(&env, &token_address);
    config1.max_members = 1;
    let result1 = client.try_initialize(&admin, &config1);
    assert!(result1.is_err());

    // max_members = 101 should fail (upper bound)
    let mut config101 = test_config(&env, &token_address);
    config101.max_members = 101;
    let result101 = client.try_initialize(&admin, &config101);
    assert!(result101.is_err());

    // max_members = 2 should succeed (minimum valid)
    let mut config2 = test_config(&env, &token_address);
    config2.max_members = 2;
    let result2 = client.try_initialize(&admin, &config2);
    assert!(result2.is_ok());
}
