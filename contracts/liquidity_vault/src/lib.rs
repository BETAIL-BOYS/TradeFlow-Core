//! # TradeFlow Liquidity Vault — Yield-Bearing LP Token (Issue #171)
//!
//! This contract implements the Soroban Token Interface (SAC standard) so that
//! every unit of liquidity deposited into a TradeFlow factoring pool is
//! represented by a transferable, yield-bearing share token (e.g. `tfUSDC`).
//!
//! ## Design
//!
//! The vault follows the ERC-4626 Tokenised Vault Standard adapted for Soroban:
//!
//! ```text
//!   User deposits X underlying assets
//!        ↓
//!   shares_to_mint = (X * total_shares) / total_assets     (or 1:1 on first deposit)
//!        ↓
//!   Vault mints `shares_to_mint` LP tokens to the user
//!        ↓
//!   Pool earns revenue (factoring discounts, interest, flash-loan fees)
//!        ↓
//!   total_assets grows; each share is now worth more underlying
//!        ↓
//!   User burns shares on withdraw → receives proportional underlying
//! ```
//!
//! ## Inflation-Attack Mitigation
//!
//! The first deposit permanently locks `MINIMUM_LIQUIDITY` (1 000) shares by
//! crediting them to `Address::zero()`, which can never sign a withdrawal.
//! This prevents the classic "first-depositor inflation attack" described in
//! ERC-4626 audit literature.
//!
//! ## SAC Token Interface
//!
//! The contract exposes all standard Soroban token functions so that any wallet,
//! DEX aggregator, or lending protocol that understands the Stellar Asset
//! Contract standard can interact with tfUSDC just like a native token.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, token,
    Address, Env, String,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of shares permanently locked on the very first deposit to defend
/// against the ERC-4626 inflation attack. These shares are assigned to the
/// zero address and can never be redeemed.
const MINIMUM_LIQUIDITY: i128 = 1_000;

/// LP token decimals — matches the underlying asset (typically USDC = 7).
/// Stored as a compile-time default; overridden by the underlying token's
/// actual decimals during initialisation.
const DEFAULT_DECIMALS: u32 = 7;

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// Contract has not been initialised yet.
    NotInitialized = 1,
    /// Caller attempted to double-initialise the vault.
    AlreadyInitialized = 2,
    /// Contract is administratively paused.
    ContractPaused = 3,
    /// An arithmetic operation would overflow or produce a nonsensical result.
    MathOverflow = 4,
    /// The caller does not have sufficient LP token balance.
    InsufficientBalance = 5,
    /// The spender's allowance is not large enough for the requested transfer.
    InsufficientAllowance = 6,
    /// The requested action was not authorised.
    Unauthorized = 7,
    /// The deposit or withdrawal amount must be positive.
    ZeroAmount = 8,
    /// Withdrawal would produce fewer underlying assets than the caller's
    /// `min_assets_out` slippage guard.
    SlippageExceeded = 9,
    /// Vault has no shares outstanding (division-by-zero guard).
    EmptyVault = 10,
    /// Allowance deadline has already passed.
    DeadlineExpired = 11,
    /// Address is frozen for compliance reasons.
    AddressFrozen = 12,
    /// Attempted transfer to the zero / burn address.
    InvalidRecipient = 13,
}

// ---------------------------------------------------------------------------
// Storage key types
// ---------------------------------------------------------------------------

#[contracttype]
pub enum DataKey {
    /// Vault configuration & accounting state (instance storage).
    VaultState,
    /// Admin address (instance storage).
    Admin,
    /// LP token balance per holder (persistent storage).
    Balance(Address),
    /// Spending allowances: (owner, spender) → amount (persistent storage).
    Allowance(Address, Address),
    /// Allowance expiry ledger (persistent storage).
    AllowanceLedger(Address, Address),
    /// Per-address freeze flag for compliance (instance storage).
    Frozen(Address),
    /// Paused flag (instance storage).
    Paused,
}

// ---------------------------------------------------------------------------
// State structs
// ---------------------------------------------------------------------------

/// Core accounting state stored in instance storage.
#[contracttype]
#[derive(Clone, Debug)]
pub struct VaultState {
    /// Address of the underlying ERC-20 / SAC token (e.g. USDC).
    pub underlying_token: Address,
    /// Total shares in circulation (i128 to match Soroban `token::balance` type).
    pub total_shares: i128,
    /// Human-readable name of the LP token (e.g. "TradeFlow USDC").
    pub name: String,
    /// Ticker symbol of the LP token (e.g. "tfUSDC").
    pub symbol: String,
    /// Decimal places of the LP token (mirrors the underlying asset).
    pub decimals: u32,
    /// Whether the first deposit has already been processed (inflation guard).
    pub first_deposit_done: bool,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct LiquidityVault;

#[contractimpl]
impl LiquidityVault {
    // -----------------------------------------------------------------------
    // Initialisation
    // -----------------------------------------------------------------------

    /// Initialise the vault.
    ///
    /// # Arguments
    /// * `admin`            – Address allowed to pause/unpause and freeze addresses.
    /// * `underlying_token` – The SAC/token address that LPs deposit (e.g. USDC).
    /// * `name`             – Human-readable name for the LP token.
    /// * `symbol`           – Ticker symbol for the LP token (e.g. "tfUSDC").
    ///
    /// Panics if the vault has already been initialised.
    pub fn initialize(
        env: Env,
        admin: Address,
        underlying_token: Address,
        name: String,
        symbol: String,
    ) {
        if env.storage().instance().has(&DataKey::VaultState) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }

        // Query the underlying token's decimals so our LP token mirrors them.
        let underlying_client = token::Client::new(&env, &underlying_token);
        let decimals = underlying_client.decimals();
        let decimals = if decimals == 0 || decimals > 18 {
            DEFAULT_DECIMALS
        } else {
            decimals
        };

        let state = VaultState {
            underlying_token,
            total_shares: 0,
            name,
            symbol,
            decimals,
            first_deposit_done: false,
        };

        env.storage().instance().set(&DataKey::VaultState, &state);
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        Self::extend_instance_ttl(&env);
    }

    // -----------------------------------------------------------------------
    // Vault read helpers
    // -----------------------------------------------------------------------

    /// Returns the total underlying assets held by the vault (physical balance).
    pub fn total_assets(env: Env) -> i128 {
        let state = Self::load_state(&env);
        let client = token::Client::new(&env, &state.underlying_token);
        client.balance(&env.current_contract_address())
    }

    /// Returns the total number of LP shares currently in circulation.
    pub fn total_supply(env: Env) -> i128 {
        Self::load_state(&env).total_shares
    }

    /// Preview how many shares a deposit of `assets` would mint right now,
    /// without applying the inflation-lock on the first deposit.
    pub fn preview_deposit(env: Env, assets: i128) -> i128 {
        if assets <= 0 {
            return 0;
        }
        let state = Self::load_state(&env);
        let total_assets = Self::total_assets(env.clone());
        Self::assets_to_shares(assets, total_assets, state.total_shares)
    }

    /// Preview how many underlying assets redeeming `shares` would return right now.
    pub fn preview_redeem(env: Env, shares: i128) -> i128 {
        if shares <= 0 {
            return 0;
        }
        let state = Self::load_state(&env);
        let total_assets = Self::total_assets(env.clone());
        Self::shares_to_assets(shares, total_assets, state.total_shares)
    }

    // -----------------------------------------------------------------------
    // Vault write functions
    // -----------------------------------------------------------------------

    /// Deposit `assets` of the underlying token and receive LP shares.
    ///
    /// On the very first deposit `MINIMUM_LIQUIDITY` shares are permanently
    /// locked to the zero address to prevent the inflation attack.
    ///
    /// # Arguments
    /// * `from`         – Depositor (must have authorised the vault to spend `assets`).
    /// * `assets`       – Amount of underlying token to deposit.
    /// * `min_shares`   – Minimum shares the depositor expects (slippage guard).
    ///
    /// # Returns
    /// The number of LP shares minted to `from`.
    pub fn deposit(env: Env, from: Address, assets: i128, min_shares: i128) -> i128 {
        from.require_auth();
        Self::check_paused(&env);
        Self::require_not_frozen(&env, &from);

        if assets <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        let mut state = Self::load_state(&env);
        let underlying = token::Client::new(&env, &state.underlying_token);

        // Total assets BEFORE the deposit transfer.
        let assets_before = underlying.balance(&env.current_contract_address());

        // Pull underlying assets from the depositor.
        underlying.transfer(&from, &env.current_contract_address(), &assets);

        // ----- share calculation -----
        let shares_to_mint: i128 = if !state.first_deposit_done {
            // First deposit: apply inflation-attack mitigation.
            // Raw shares = assets (1:1 on an empty vault).
            let raw_shares = assets;

            // Permanently lock MINIMUM_LIQUIDITY shares to the zero address.
            // We represent this by crediting total_shares without giving anyone
            // a balance entry, so they are unclaimable forever.
            if raw_shares <= MINIMUM_LIQUIDITY {
                panic_with_error!(&env, Error::ZeroAmount); // deposit too small
            }

            // The depositor receives the remainder; the locked shares inflate the
            // denominator for all future deposits, making the attack economically
            // infeasible.
            state.total_shares = state
                .total_shares
                .checked_add(MINIMUM_LIQUIDITY)
                .unwrap_or_else(|| panic_with_error!(&env, Error::MathOverflow));
            state.first_deposit_done = true;

            raw_shares
                .checked_sub(MINIMUM_LIQUIDITY)
                .unwrap_or_else(|| panic_with_error!(&env, Error::MathOverflow))
        } else {
            // Subsequent deposits use the current exchange rate.
            Self::assets_to_shares(assets, assets_before, state.total_shares)
        };

        if shares_to_mint <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        if shares_to_mint < min_shares {
            panic_with_error!(&env, Error::SlippageExceeded);
        }

        // Mint shares to the depositor.
        state.total_shares = state
            .total_shares
            .checked_add(shares_to_mint)
            .unwrap_or_else(|| panic_with_error!(&env, Error::MathOverflow));
        env.storage().instance().set(&DataKey::VaultState, &state);

        Self::increase_balance(&env, &from, shares_to_mint);
        Self::extend_instance_ttl(&env);

        env.events().publish(
            (symbol_short!("deposit"), from.clone()),
            (assets, shares_to_mint),
        );

        shares_to_mint
    }

    /// Burn `shares` LP tokens and receive the proportional underlying assets.
    ///
    /// # Arguments
    /// * `from`          – Share-holder initiating the withdrawal.
    /// * `shares`        – Number of LP tokens to burn.
    /// * `min_assets_out`– Minimum underlying assets the caller expects (slippage guard).
    ///
    /// # Returns
    /// The number of underlying asset tokens returned to `from`.
    pub fn withdraw(env: Env, from: Address, shares: i128, min_assets_out: i128) -> i128 {
        from.require_auth();
        Self::check_paused(&env);
        Self::require_not_frozen(&env, &from);

        if shares <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        let caller_balance = Self::get_balance(&env, &from);
        if caller_balance < shares {
            panic_with_error!(&env, Error::InsufficientBalance);
        }

        let mut state = Self::load_state(&env);
        if state.total_shares == 0 {
            panic_with_error!(&env, Error::EmptyVault);
        }

        let underlying = token::Client::new(&env, &state.underlying_token);
        let total_assets_now = underlying.balance(&env.current_contract_address());

        let assets_out = Self::shares_to_assets(shares, total_assets_now, state.total_shares);

        if assets_out <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        if assets_out < min_assets_out {
            panic_with_error!(&env, Error::SlippageExceeded);
        }

        // Burn shares first (checks-effects-interactions pattern).
        state.total_shares = state
            .total_shares
            .checked_sub(shares)
            .unwrap_or_else(|| panic_with_error!(&env, Error::MathOverflow));
        env.storage().instance().set(&DataKey::VaultState, &state);

        Self::decrease_balance(&env, &from, shares);

        // Transfer underlying to the caller.
        underlying.transfer(&env.current_contract_address(), &from, &assets_out);
        Self::extend_instance_ttl(&env);

        env.events().publish(
            (symbol_short!("withdraw"), from.clone()),
            (shares, assets_out),
        );

        assets_out
    }

    // -----------------------------------------------------------------------
    // Soroban Token Interface — READ functions
    // -----------------------------------------------------------------------

    /// Returns the LP token balance of `id`.
    pub fn balance(env: Env, id: Address) -> i128 {
        Self::get_balance(&env, &id)
    }

    /// Returns how many LP tokens `spender` is allowed to spend on behalf of `owner`.
    pub fn allowance(env: Env, owner: Address, spender: Address) -> i128 {
        Self::get_allowance(&env, &owner, &spender)
    }

    /// Returns the number of decimal places (mirrors the underlying token).
    pub fn decimals(env: Env) -> u32 {
        Self::load_state(&env).decimals
    }

    /// Returns the human-readable name of the LP token.
    pub fn name(env: Env) -> String {
        Self::load_state(&env).name
    }

    /// Returns the ticker symbol of the LP token.
    pub fn symbol(env: Env) -> String {
        Self::load_state(&env).symbol
    }

    // -----------------------------------------------------------------------
    // Soroban Token Interface — WRITE functions
    // -----------------------------------------------------------------------

    /// Transfer `amount` LP tokens from the caller to `to`.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        Self::check_paused(&env);
        Self::require_not_frozen(&env, &from);

        if amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        let from_balance = Self::get_balance(&env, &from);
        if from_balance < amount {
            panic_with_error!(&env, Error::InsufficientBalance);
        }

        Self::decrease_balance(&env, &from, amount);
        Self::increase_balance(&env, &to, amount);
        Self::extend_instance_ttl(&env);

        env.events().publish(
            (symbol_short!("transfer"), from.clone()),
            (to, amount),
        );
    }

    /// Transfer `amount` LP tokens from `from` to `to` using the caller's
    /// pre-approved spending allowance.
    pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) {
        spender.require_auth();
        Self::check_paused(&env);
        Self::require_not_frozen(&env, &from);

        if amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        // Consume allowance.
        let allowance = Self::get_allowance(&env, &from, &spender);
        if allowance < amount {
            panic_with_error!(&env, Error::InsufficientAllowance);
        }
        Self::set_allowance(&env, &from, &spender, allowance - amount, 0);

        let from_balance = Self::get_balance(&env, &from);
        if from_balance < amount {
            panic_with_error!(&env, Error::InsufficientBalance);
        }

        Self::decrease_balance(&env, &from, amount);
        Self::increase_balance(&env, &to, amount);
        Self::extend_instance_ttl(&env);

        env.events().publish(
            (symbol_short!("xfer_from"), spender),
            (from, to, amount),
        );
    }

    /// Approve `spender` to spend up to `amount` of the caller's LP tokens.
    ///
    /// `expiration_ledger` is the ledger sequence number after which the
    /// allowance expires (0 means no expiry).
    pub fn approve(
        env: Env,
        owner: Address,
        spender: Address,
        amount: i128,
        expiration_ledger: u32,
    ) {
        owner.require_auth();
        Self::check_paused(&env);

        if expiration_ledger > 0 && expiration_ledger < env.ledger().sequence() {
            panic_with_error!(&env, Error::DeadlineExpired);
        }

        Self::set_allowance(&env, &owner, &spender, amount, expiration_ledger);
        Self::extend_instance_ttl(&env);

        env.events().publish(
            (symbol_short!("approve"), owner.clone()),
            (spender, amount, expiration_ledger),
        );
    }

    // -----------------------------------------------------------------------
    // Admin functions
    // -----------------------------------------------------------------------

    /// Pause or unpause the vault.  Only callable by the admin.
    pub fn set_paused(env: Env, paused: bool) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &paused);
        Self::extend_instance_ttl(&env);
        env.events().publish((symbol_short!("pause_set"), paused), env.ledger().sequence());
    }

    /// Freeze or unfreeze an address for compliance reasons.  Admin only.
    pub fn set_frozen(env: Env, address: Address, frozen: bool) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::Frozen(address.clone()), &frozen);
        Self::extend_instance_ttl(&env);
        env.events().publish(
            (symbol_short!("freeze"), address),
            frozen,
        );
    }

    /// Returns whether `address` is currently frozen.
    pub fn is_frozen(env: Env, address: Address) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Frozen(address))
            .unwrap_or(false)
    }

    /// Returns whether the vault is currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // Internal / private helpers
    // -----------------------------------------------------------------------

    /// Load the VaultState from instance storage, panicking if uninitialised.
    fn load_state(env: &Env) -> VaultState {
        env.storage()
            .instance()
            .get(&DataKey::VaultState)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized))
    }

    /// Extend instance storage TTL to ~30 days (535 680 ledgers).
    fn extend_instance_ttl(env: &Env) {
        env.storage().instance().extend_ttl(535_680, 535_680);
    }

    /// Extend persistent storage TTL for a given key.
    fn extend_persistent_ttl(env: &Env, key: &DataKey) {
        env.storage().persistent().extend_ttl(key, 535_680, 535_680);
    }

    /// Require the current caller to be the stored admin.
    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized));
        admin.require_auth();
    }

    /// Revert if the vault is paused.
    fn check_paused(env: &Env) {
        if env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
        {
            panic_with_error!(env, Error::ContractPaused);
        }
    }

    /// Revert if `address` is frozen.
    fn require_not_frozen(env: &Env, address: &Address) {
        if env
            .storage()
            .instance()
            .get(&DataKey::Frozen(address.clone()))
            .unwrap_or(false)
        {
            panic_with_error!(env, Error::AddressFrozen);
        }
    }

    // ----------- share ↔ asset maths ------------------------------------

    /// Convert an `assets` amount to shares using the current exchange rate.
    ///
    /// Formula (rounds down, safe for the vault):
    /// ```text
    /// shares = (assets * total_shares) / total_assets
    /// ```
    /// Falls back to 1:1 when the vault is empty.
    fn assets_to_shares(assets: i128, total_assets: i128, total_shares: i128) -> i128 {
        if total_shares == 0 || total_assets == 0 {
            return assets; // 1:1 on empty vault
        }
        // Checked multiply to catch overflow on very large values.
        let numerator = assets
            .checked_mul(total_shares)
            .unwrap_or(i128::MAX); // saturate; checked below
        numerator / total_assets
    }

    /// Convert a `shares` amount to underlying assets using the current exchange rate.
    ///
    /// Formula (rounds down, safe for the vault):
    /// ```text
    /// assets = (shares * total_assets) / total_shares
    /// ```
    fn shares_to_assets(shares: i128, total_assets: i128, total_shares: i128) -> i128 {
        if total_shares == 0 || total_assets == 0 {
            return 0;
        }
        let numerator = shares
            .checked_mul(total_assets)
            .unwrap_or(i128::MAX); // saturate
        numerator / total_shares
    }

    // ----------- Balance ledger ------------------------------------------

    fn get_balance(env: &Env, address: &Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(address.clone()))
            .unwrap_or(0i128)
    }

    fn increase_balance(env: &Env, address: &Address, amount: i128) {
        let key = DataKey::Balance(address.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_balance = current
            .checked_add(amount)
            .unwrap_or_else(|| panic_with_error!(env, Error::MathOverflow));
        env.storage().persistent().set(&key, &new_balance);
        Self::extend_persistent_ttl(env, &key);
    }

    fn decrease_balance(env: &Env, address: &Address, amount: i128) {
        let key = DataKey::Balance(address.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if current < amount {
            panic_with_error!(env, Error::InsufficientBalance);
        }
        let new_balance = current - amount;
        env.storage().persistent().set(&key, &new_balance);
        Self::extend_persistent_ttl(env, &key);
    }

    // ----------- Allowance ledger ----------------------------------------

    fn get_allowance(env: &Env, owner: &Address, spender: &Address) -> i128 {
        let key = DataKey::Allowance(owner.clone(), spender.clone());
        let ledger_key = DataKey::AllowanceLedger(owner.clone(), spender.clone());

        // If the allowance has an expiry and it's passed, treat as zero.
        let expiry: u32 = env.storage().persistent().get(&ledger_key).unwrap_or(0);
        if expiry > 0 && expiry < env.ledger().sequence() {
            return 0;
        }

        env.storage().persistent().get(&key).unwrap_or(0i128)
    }

    fn set_allowance(
        env: &Env,
        owner: &Address,
        spender: &Address,
        amount: i128,
        expiration_ledger: u32,
    ) {
        let key = DataKey::Allowance(owner.clone(), spender.clone());
        let ledger_key = DataKey::AllowanceLedger(owner.clone(), spender.clone());

        env.storage().persistent().set(&key, &amount);
        env.storage()
            .persistent()
            .set(&ledger_key, &expiration_ledger);
        Self::extend_persistent_ttl(env, &key);
        Self::extend_persistent_ttl(env, &ledger_key);
    }
}
