#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol};

#[contracttype]
pub enum DataKey {
    Admin,
    TokenAddress, // The address of the USDC token
}

#[contract]
pub struct LendingPool;

#[contractimpl]
impl LendingPool {
    // 1. INITIALIZE: Set the token we are lending (e.g., USDC)
    pub fn init(env: Env, admin: Address, token_address: Address) {
        // Simple check to ensure we don't overwrite
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TokenAddress, &token_address);
    }

    // 2. DEPOSIT: LPs add capital to the pool
    pub fn deposit(env: Env, from: Address, amount: i128) {
        from.require_auth();

        let token_addr: Address = env.storage().instance().get(&DataKey::TokenAddress).expect("Not initialized");
        let client = token::Client::new(&env, &token_addr);

        // Transfer from User -> Contract
        client.transfer(&from, &env.current_contract_address(), &amount);
        
        // (In a real app, we would mint "Pool Share Tokens" here)
        env.events().publish((Symbol::new(&env, "deposit"), from), amount);
    }

    // 3. BORROW: Borrow against an invoice (Simplified)
    pub fn borrow(env: Env, borrower: Address, amount: i128) {
        borrower.require_auth();

        // 1. Check if the pool has enough funds
        let token_addr: Address = env.storage().instance().get(&DataKey::TokenAddress).expect("Not initialized");
        let client = token::Client::new(&env, &token_addr);
        
        let pool_balance = client.balance(&env.current_contract_address());
        if amount > pool_balance {
            panic!("Insufficient pool liquidity");
        }

        // 2. Transfer funds Contract -> Borrower
        client.transfer(&env.current_contract_address(), &borrower, &amount);

        env.events().publish((Symbol::new(&env, "borrow"), borrower), amount);
    }

    // 4. VIEW: Check contract balance
    pub fn get_pool_balance(env: Env) -> i128 {
        let token_addr: Address = env.storage().instance().get(&DataKey::TokenAddress).expect("Not initialized");
        let client = token::Client::new(&env, &token_addr);
        client.balance(&env.current_contract_address())
    }
}