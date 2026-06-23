//! Unit tests for the LiquidityVault contract.
//!
//! Covers:
//! - Basic deposit / withdraw round-trip
//! - Inflation-attack mitigation (first 1 000 shares locked)
//! - Yield accrual: share value increases as pool earns revenue
//! - Multi-depositor proportional withdrawal
//! - SAC token interface: transfer, transfer_from, approve, allowance
//! - Admin controls: pause, freeze

#![cfg(test)]

extern crate std;

use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal, String,
};

use crate::{Error, LiquidityVault, LiquidityVaultClient};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Deploy a mock USDC SAC token and return its contract ID plus an admin that
/// can mint arbitrary balances.
fn create_token<'a>(env: &Env, admin: &Address) -> (Address, StellarAssetClient<'a>) {
    let contract_id = env.register_stellar_asset_contract_v2(admin.clone());
    let sac = StellarAssetClient::new(env, &contract_id.address());
    (contract_id.address(), sac)
}

/// Deploy the vault and return its client.
fn deploy_vault<'a>(
    env: &Env,
    admin: &Address,
    underlying: &Address,
) -> LiquidityVaultClient<'a> {
    let vault_id = env.register_contract(None, LiquidityVault);
    let client = LiquidityVaultClient::new(env, &vault_id);
    client.initialize(
        admin,
        underlying,
        &String::from_str(env, "TradeFlow USDC"),
        &String::from_str(env, "tfUSDC"),
    );
    client
}

/// Fund `user` with `amount` of `token` via the SAC mint authority.
fn fund(sac: &StellarAssetClient, user: &Address, amount: i128) {
    sac.mint(user, &amount);
}

/// Give the vault contract infinite allowance from `user` over their `token`.
fn approve_vault(env: &Env, token: &TokenClient, user: &Address, vault: &Address, amount: i128) {
    token.approve(user, vault, &amount, &(env.ledger().sequence() + 535_680));
}

// ---------------------------------------------------------------------------
// 1. Basic deposit / withdraw
// ---------------------------------------------------------------------------

#[test]
fn test_basic_deposit_and_withdraw() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let alice = Address::generate(&env);

    let (usdc_addr, usdc_sac) = create_token(&env, &admin);
    let usdc = TokenClient::new(&env, &usdc_addr);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    // Mint 10_000 USDC to Alice and approve the vault.
    fund(&usdc_sac, &alice, 10_000);
    approve_vault(&env, &usdc, &alice, &vault.address, 10_000);

    // --- First deposit: 5 001 units (> MINIMUM_LIQUIDITY of 1 000) ---
    let shares = vault.deposit(&alice, &5_001, &1);
    // Alice gets 5 001 - 1 000 = 4 001 shares; 1 000 locked permanently.
    assert_eq!(shares, 4_001);
    assert_eq!(vault.balance(&alice), 4_001);
    assert_eq!(vault.total_supply(), 5_001); // 4 001 alice + 1 000 locked

    // --- Withdraw all Alice's shares ---
    // total_assets = 5_001, total_shares = 5_001 → rate is 1:1
    let assets_out = vault.withdraw(&alice, &4_001, &1);
    assert_eq!(assets_out, 4_001);
    assert_eq!(vault.balance(&alice), 0);
    // 1 000 locked shares remain; 1 000 underlying remain as well.
    assert_eq!(vault.total_supply(), 1_000);
    assert_eq!(vault.total_assets(), 1_000);
}

// ---------------------------------------------------------------------------
// 2. Inflation-attack mitigation
// ---------------------------------------------------------------------------

#[test]
fn test_inflation_attack_mitigation() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let victim = Address::generate(&env);

    let (usdc_addr, usdc_sac) = create_token(&env, &admin);
    let usdc = TokenClient::new(&env, &usdc_addr);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    // Attacker tries the classic inflation attack:
    //   1. Deposit 1 (or a tiny amount) to become the only share-holder.
    //   2. Donate a large amount directly to the vault.
    //   3. Victim deposits "1"; receives 0 shares due to rounding.
    //   4. Attacker withdraws, stealing the victim's deposit.
    //
    // The MINIMUM_LIQUIDITY lock makes step 3 impossible because the locked
    // shares force the denominator to be at least 1 000, making rounding
    // harmless for victim deposits above dust level.

    fund(&usdc_sac, &attacker, 2_000_000);
    approve_vault(&env, &usdc, &attacker, &vault.address, 2_000_000);

    // Attacker deposits 1 001 (just over MINIMUM_LIQUIDITY so it doesn't panic).
    let attacker_shares = vault.deposit(&attacker, &1_001, &1);
    // attacker_shares = 1 001 - 1 000 = 1
    assert_eq!(attacker_shares, 1);
    assert_eq!(vault.balance(&attacker), 1);

    // Attacker donates 1_000_000 USDC directly to the vault (no shares minted).
    // total_assets is now 1_001 + 1_000_000 = 1_001_001
    // total_shares = 1_001
    usdc.transfer(&attacker, &vault.address, &1_000_000);
    assert_eq!(vault.total_assets(), 1_001_001);

    // Victim deposits 2_001 USDC.
    fund(&usdc_sac, &victim, 2_001);
    approve_vault(&env, &usdc, &victim, &vault.address, 2_001);

    // shares_to_mint = 2_001 * 1_001 / 1_001_001 ≈ 2 shares (not 0)
    // Without MINIMUM_LIQUIDITY the denominator would be 1 (only attacker's
    // 1 share), giving victim 0 shares — the attack succeeds.
    // With the lock the victim always gets a non-trivial share count.
    let victim_shares = vault.deposit(&victim, &2_001, &1);
    assert!(victim_shares >= 1, "victim must receive at least 1 share");
    assert_eq!(vault.balance(&victim), victim_shares);
}

// ---------------------------------------------------------------------------
// 3. Yield accrual
// ---------------------------------------------------------------------------

#[test]
fn test_yield_accrual_increases_share_value() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    let (usdc_addr, usdc_sac) = create_token(&env, &admin);
    let usdc = TokenClient::new(&env, &usdc_addr);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    // Alice deposits 10_000 USDC.
    fund(&usdc_sac, &alice, 10_000);
    approve_vault(&env, &usdc, &alice, &vault.address, 10_000);
    let alice_shares = vault.deposit(&alice, &10_000, &1);

    // Simulate yield: 1 000 USDC flows into the vault (factoring revenue).
    // total_assets is now 11_000; total_shares unchanged.
    fund(&usdc_sac, &admin, 1_000);
    usdc.transfer(&admin, &vault.address, &1_000);

    // Bob deposits 5 500 USDC at the new (higher) rate.
    fund(&usdc_sac, &bob, 5_500);
    approve_vault(&env, &usdc, &bob, &vault.address, 5_500);
    let bob_shares = vault.deposit(&bob, &5_500, &1);

    // total_assets = 11_000 + 5_500 = 16_500 before Bob's deposit is counted
    // but the preview_deposit logic uses the BEFORE balance:
    //   bob_shares = 5_500 * total_shares_before / 11_000
    // Bob should receive fewer shares per USDC than Alice did, proving yield.
    // Alice got ~1 share per 1 USDC; Bob gets ~1 share per ~1.2 USDC.
    assert!(
        bob_shares < alice_shares,
        "Bob should receive fewer shares than Alice because the exchange rate has risen"
    );

    // Alice redeems her shares — she should receive more than her original 10 000.
    let alice_assets_out = vault.withdraw(&alice, &alice_shares, &1);
    assert!(
        alice_assets_out > 10_000,
        "Alice should profit from the yield accrual"
    );
}

// ---------------------------------------------------------------------------
// 4. Multi-depositor proportional withdrawal
// ---------------------------------------------------------------------------

#[test]
fn test_multi_depositor_proportional_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let carol = Address::generate(&env);

    let (usdc_addr, usdc_sac) = create_token(&env, &admin);
    let usdc = TokenClient::new(&env, &usdc_addr);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    // Alice deposits first (pays the MINIMUM_LIQUIDITY tax).
    fund(&usdc_sac, &alice, 10_000);
    approve_vault(&env, &usdc, &alice, &vault.address, 10_000);
    let alice_shares = vault.deposit(&alice, &10_000, &1);

    // Bob deposits the same amount.
    fund(&usdc_sac, &bob, 10_000);
    approve_vault(&env, &usdc, &bob, &vault.address, 10_000);
    let bob_shares = vault.deposit(&bob, &10_000, &1);

    // Carol deposits twice as much.
    fund(&usdc_sac, &carol, 20_000);
    approve_vault(&env, &usdc, &carol, &vault.address, 20_000);
    let carol_shares = vault.deposit(&carol, &20_000, &1);

    // Carol should hold roughly twice the shares of Bob.
    // (Alice paid an extra 1 000 lock, so her shares are slightly less than
    //  Bob's, and Carol's are roughly 2× Bob's.)
    assert!(carol_shares > bob_shares, "Carol deposited 2× Bob");

    // Add 4 000 USDC of yield.
    fund(&usdc_sac, &admin, 4_000);
    usdc.transfer(&admin, &vault.address, &4_000);

    // Everyone withdraws; nobody receives zero.
    let alice_out = vault.withdraw(&alice, &alice_shares, &1);
    let bob_out = vault.withdraw(&bob, &bob_shares, &1);
    let carol_out = vault.withdraw(&carol, &carol_shares, &1);

    assert!(alice_out > 0, "Alice should receive underlying assets");
    assert!(bob_out > 0, "Bob should receive underlying assets");
    assert!(carol_out > 0, "Carol should receive underlying assets");

    // Carol should receive roughly twice what Bob does.
    // Use a 10% tolerance to account for integer rounding.
    let carol_expected = bob_out * 2;
    let diff = if carol_out > carol_expected {
        carol_out - carol_expected
    } else {
        carol_expected - carol_out
    };
    assert!(
        diff * 10 <= carol_expected,
        "Carol's payout should be within 10% of 2× Bob's (carol={}, 2×bob={})",
        carol_out,
        carol_expected
    );
}

// ---------------------------------------------------------------------------
// 5. SAC token interface — transfer
// ---------------------------------------------------------------------------

#[test]
fn test_transfer_lp_tokens() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    let (usdc_addr, usdc_sac) = create_token(&env, &admin);
    let usdc = TokenClient::new(&env, &usdc_addr);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    fund(&usdc_sac, &alice, 5_001);
    approve_vault(&env, &usdc, &alice, &vault.address, 5_001);
    let alice_shares = vault.deposit(&alice, &5_001, &1);

    // Alice transfers half her shares to Bob.
    let half = alice_shares / 2;
    vault.transfer(&alice, &bob, &half);

    assert_eq!(vault.balance(&alice), alice_shares - half);
    assert_eq!(vault.balance(&bob), half);
}

// ---------------------------------------------------------------------------
// 6. SAC token interface — approve / transfer_from
// ---------------------------------------------------------------------------

#[test]
fn test_approve_and_transfer_from() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let alice = Address::generate(&env);
    let spender = Address::generate(&env);
    let bob = Address::generate(&env);

    let (usdc_addr, usdc_sac) = create_token(&env, &admin);
    let usdc = TokenClient::new(&env, &usdc_addr);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    fund(&usdc_sac, &alice, 5_001);
    approve_vault(&env, &usdc, &alice, &vault.address, 5_001);
    let alice_shares = vault.deposit(&alice, &5_001, &1);

    // Alice approves `spender` to move 1 000 of her LP tokens.
    let approval = 1_000i128;
    vault.approve(&alice, &spender, &approval, &(env.ledger().sequence() + 100));
    assert_eq!(vault.allowance(&alice, &spender), approval);

    // Spender moves 500 from Alice to Bob.
    vault.transfer_from(&spender, &alice, &bob, &500);
    assert_eq!(vault.balance(&bob), 500);
    assert_eq!(vault.allowance(&alice, &spender), 500); // 1000 - 500

    // Alice's balance reduced by the transferred amount.
    assert_eq!(vault.balance(&alice), alice_shares - 500);
}

// ---------------------------------------------------------------------------
// 7. Insufficient balance / allowance
// ---------------------------------------------------------------------------

#[test]
#[should_panic]
fn test_transfer_insufficient_balance_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    let (usdc_addr, usdc_sac) = create_token(&env, &admin);
    let usdc = TokenClient::new(&env, &usdc_addr);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    fund(&usdc_sac, &alice, 2_000);
    approve_vault(&env, &usdc, &alice, &vault.address, 2_000);
    vault.deposit(&alice, &2_000, &1);

    // Try to transfer more shares than Alice holds.
    vault.transfer(&alice, &bob, &9_999_999);
}

#[test]
#[should_panic]
fn test_transfer_from_insufficient_allowance_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let alice = Address::generate(&env);
    let spender = Address::generate(&env);
    let bob = Address::generate(&env);

    let (usdc_addr, usdc_sac) = create_token(&env, &admin);
    let usdc = TokenClient::new(&env, &usdc_addr);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    fund(&usdc_sac, &alice, 5_001);
    approve_vault(&env, &usdc, &alice, &vault.address, 5_001);
    vault.deposit(&alice, &5_001, &1);

    // Approve only 10 but try to transfer 500.
    vault.approve(&alice, &spender, &10, &(env.ledger().sequence() + 100));
    vault.transfer_from(&spender, &alice, &bob, &500);
}

// ---------------------------------------------------------------------------
// 8. Admin pause
// ---------------------------------------------------------------------------

#[test]
#[should_panic]
fn test_deposit_when_paused_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let alice = Address::generate(&env);

    let (usdc_addr, usdc_sac) = create_token(&env, &admin);
    let usdc = TokenClient::new(&env, &usdc_addr);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    vault.set_paused(&true);

    fund(&usdc_sac, &alice, 5_001);
    approve_vault(&env, &usdc, &alice, &vault.address, 5_001);
    vault.deposit(&alice, &5_001, &1); // Should panic: ContractPaused
}

// ---------------------------------------------------------------------------
// 9. Address freeze
// ---------------------------------------------------------------------------

#[test]
#[should_panic]
fn test_deposit_frozen_address_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let alice = Address::generate(&env);

    let (usdc_addr, usdc_sac) = create_token(&env, &admin);
    let usdc = TokenClient::new(&env, &usdc_addr);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    vault.set_frozen(&alice, &true);

    fund(&usdc_sac, &alice, 5_001);
    approve_vault(&env, &usdc, &alice, &vault.address, 5_001);
    vault.deposit(&alice, &5_001, &1); // Should panic: AddressFrozen
}

// ---------------------------------------------------------------------------
// 10. Slippage guard on deposit
// ---------------------------------------------------------------------------

#[test]
#[should_panic]
fn test_deposit_slippage_guard_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let alice = Address::generate(&env);

    let (usdc_addr, usdc_sac) = create_token(&env, &admin);
    let usdc = TokenClient::new(&env, &usdc_addr);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    fund(&usdc_sac, &alice, 5_001);
    approve_vault(&env, &usdc, &alice, &vault.address, 5_001);

    // Alice expects 999_999 shares but will only get ~4_001 — slippage guard fires.
    vault.deposit(&alice, &5_001, &999_999);
}

// ---------------------------------------------------------------------------
// 11. Token metadata
// ---------------------------------------------------------------------------

#[test]
fn test_token_metadata() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (usdc_addr, _) = create_token(&env, &admin);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    assert_eq!(vault.name(), String::from_str(&env, "TradeFlow USDC"));
    assert_eq!(vault.symbol(), String::from_str(&env, "tfUSDC"));
    // Decimals mirror the underlying token (default 7 for SAC).
    assert!(vault.decimals() > 0);
}

// ---------------------------------------------------------------------------
// 12. Deposit too small (below MINIMUM_LIQUIDITY) panics on first deposit
// ---------------------------------------------------------------------------

#[test]
#[should_panic]
fn test_first_deposit_below_minimum_liquidity_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let alice = Address::generate(&env);

    let (usdc_addr, usdc_sac) = create_token(&env, &admin);
    let usdc = TokenClient::new(&env, &usdc_addr);
    let vault = deploy_vault(&env, &admin, &usdc_addr);

    fund(&usdc_sac, &alice, 500);
    approve_vault(&env, &usdc, &alice, &vault.address, 500);

    // Deposit of 500 is less than MINIMUM_LIQUIDITY (1 000) — must panic.
    vault.deposit(&alice, &500, &1);
}
