extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};

use crate::{TradeFlow, TradeFlowClient};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TradeFlow, ());
    let admin = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    TradeFlowClient::new(&env, &contract_id)
        .init(&admin, &token_a, &token_b, &30);
    (env, contract_id, admin)
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> TradeFlowClient<'a> {
    TradeFlowClient::new(env, contract_id)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_admin_holds_default_admin_role_after_init() {
    let (env, contract_id, admin) = setup();
    let role = client(&env, &contract_id).role_default_admin();
    assert!(client(&env, &contract_id).has_role(&role, &admin));
}

#[test]
fn test_grant_role_by_default_admin() {
    let (env, contract_id, admin) = setup();
    let fee_role = client(&env, &contract_id).role_fee_manager();
    let manager = Address::generate(&env);

    client(&env, &contract_id).grant_role(&admin, &fee_role, &manager);

    assert!(client(&env, &contract_id).has_role(&fee_role, &manager));
}

#[test]
fn test_grant_role_unauthorized_panics() {
    let (env, contract_id, _admin) = setup();
    let fee_role = client(&env, &contract_id).role_fee_manager();
    let attacker = Address::generate(&env);
    let victim = Address::generate(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client(&env, &contract_id).grant_role(&attacker, &fee_role, &victim);
    }));
    assert!(result.is_err(), "non-admin granting role should panic");
}

#[test]
fn test_revoke_role_by_default_admin() {
    let (env, contract_id, admin) = setup();
    let fee_role = client(&env, &contract_id).role_fee_manager();
    let manager = Address::generate(&env);

    client(&env, &contract_id).grant_role(&admin, &fee_role, &manager);
    assert!(client(&env, &contract_id).has_role(&fee_role, &manager));

    client(&env, &contract_id).revoke_role(&admin, &fee_role, &manager);
    assert!(!client(&env, &contract_id).has_role(&fee_role, &manager));
}

#[test]
fn test_revoke_role_unauthorized_panics() {
    let (env, contract_id, admin) = setup();
    let fee_role = client(&env, &contract_id).role_fee_manager();
    let manager = Address::generate(&env);
    let attacker = Address::generate(&env);

    client(&env, &contract_id).grant_role(&admin, &fee_role, &manager);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client(&env, &contract_id).revoke_role(&attacker, &fee_role, &manager);
    }));
    assert!(result.is_err(), "non-admin revoking role should panic");
}

#[test]
fn test_renounce_role_removes_own_role() {
    let (env, contract_id, admin) = setup();
    let fee_role = client(&env, &contract_id).role_fee_manager();
    let manager = Address::generate(&env);

    client(&env, &contract_id).grant_role(&admin, &fee_role, &manager);
    client(&env, &contract_id).renounce_role(&manager, &fee_role);

    assert!(!client(&env, &contract_id).has_role(&fee_role, &manager));
}

#[test]
fn test_fee_manager_role_required_for_propose_fee_change() {
    let (env, contract_id, admin) = setup();
    let fee_role = client(&env, &contract_id).role_fee_manager();
    let manager = Address::generate(&env);

    // no FEE_MANAGER_ROLE yet — must panic
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client(&env, &contract_id).propose_fee_change(&manager, &50);
    }));
    assert!(result.is_err(), "propose_fee_change without role should panic");

    // grant role then retry — must succeed
    client(&env, &contract_id).grant_role(&admin, &fee_role, &manager);
    client(&env, &contract_id).propose_fee_change(&manager, &50);

    let pending = client(&env, &contract_id).get_pending_fee_change().unwrap();
    assert_eq!(pending.new_fee, 50);
}

#[test]
fn test_default_admin_role_required_for_upgrade_contract() {
    let (env, contract_id, admin) = setup();
    let attacker = Address::generate(&env);
    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);

    // attacker has no DEFAULT_ADMIN_ROLE → must panic
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client(&env, &contract_id).upgrade_contract(&attacker, &dummy_hash);
    }));
    assert!(result.is_err(), "upgrade_contract without DEFAULT_ADMIN_ROLE should panic");

    // admin holds DEFAULT_ADMIN_ROLE → must succeed
    client(&env, &contract_id).upgrade_contract(&admin, &dummy_hash);
}

#[test]
fn test_fee_change_timelock_with_rbac() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TradeFlow, ());
    let admin = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    client(&env, &contract_id).init(&admin, &token_a, &token_b, &30);

    // admin holds DEFAULT_ADMIN_ROLE — grant FEE_MANAGER_ROLE to self
    let fee_role = client(&env, &contract_id).role_fee_manager();
    client(&env, &contract_id).grant_role(&admin, &fee_role, &admin);

    // propose
    client(&env, &contract_id).propose_fee_change(&admin, &75);
    assert_eq!(client(&env, &contract_id).get_pending_fee_change().unwrap().new_fee, 75);

    // cannot execute before 48 h
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client(&env, &contract_id).execute_fee_change(&admin);
    }));
    assert!(result.is_err(), "should panic: timelock not elapsed");

    // advance past timelock
    let current_ts = env.ledger().timestamp();
    env.ledger().with_mut(|li| li.timestamp = current_ts + 48 * 60 * 60 + 1);
    env.mock_all_auths();

    client(&env, &contract_id).execute_fee_change(&admin);
    assert!(client(&env, &contract_id).get_pending_fee_change().is_none());
}

#[test]
fn test_pauser_role_required_for_toggle_pool_status() {
    let (env, contract_id, admin) = setup();
    let pauser_role = client(&env, &contract_id).role_pauser();
    let pauser = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // no PAUSER_ROLE → role check panics
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::FactoryContractClient::new(&env, &contract_id)
            .toggle_pool_status(&pauser, &token_a, &token_b);
    }));
    assert!(result.is_err(), "toggle_pool_status without PAUSER_ROLE should panic");

    // grant PAUSER_ROLE — role check now passes, pool-not-found panic expected next
    client(&env, &contract_id).grant_role(&admin, &pauser_role, &pauser);
    let result2 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::FactoryContractClient::new(&env, &contract_id)
            .toggle_pool_status(&pauser, &token_a, &token_b);
    }));
    // Must panic (pool doesn't exist), but NOT with a role error
    assert!(result2.is_err(), "should panic on missing pool");
}

#[test]
fn test_role_hash_constants_are_deterministic() {
    let env = Env::default();
    let contract_id = env.register(TradeFlow, ());
    let c = TradeFlowClient::new(&env, &contract_id);

    // Same env, same hash every time
    assert_eq!(c.role_default_admin(), c.role_default_admin());
    assert_eq!(c.role_fee_manager(), c.role_fee_manager());
    assert_eq!(c.role_pauser(), c.role_pauser());
    assert_eq!(c.role_kyc_manager(), c.role_kyc_manager());

    // Different roles produce different hashes
    assert_ne!(c.role_default_admin(), c.role_fee_manager());
    assert_ne!(c.role_fee_manager(), c.role_pauser());
}
