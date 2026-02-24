#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, BytesN, Env, Symbol};

mod tests;

#[contracttype]
#[derive(Clone)]
pub struct Loan {
    pub id: u64,
    pub borrower: Address,
    pub invoice_id: u64,
    pub principal: i128,
    pub interest: i128,
    pub start_time: u64,
    pub due_date: u64,
    pub is_repaid: bool,
    pub is_defaulted: bool,
}

#[contracttype]
pub enum LoanStatus {
    Active,
    Repaid,
    Defaulted,
}

#[contracttype]
pub enum DataKey {
    Admin,
    TokenAddress,  // The address of the USDC token
    Paused,        // Contract pause state
    Loan(u64),     // Maps ID -> Loan
    LoanId,        // Tracks the next available loan ID
    BackendPubkey, // Backend public key for signature verification
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
        env.storage()
            .instance()
            .set(&DataKey::TokenAddress, &token_address);
        env.storage().instance().set(&DataKey::Paused, &false);
    }

    // Helper function to check if contract is paused
    fn check_paused(env: &Env) {
        if env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
        {
            panic!("CONTRACT_PAUSED");
        }
    }

    // Helper function to check admin authorization
    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();
    }

    // PAUSE CONTROL: Set contract pause state (admin only)
    pub fn set_paused(env: Env, paused: bool) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &paused);
        env.events().publish(
            (Symbol::new(&env, "pause_set"), paused),
            env.ledger().sequence(),
        );
    }

    // GET PAUSE STATE: Check if contract is paused
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    // 2. DEPOSIT: LPs add capital to the pool
    pub fn deposit(env: Env, from: Address, amount: i128) {
        Self::check_paused(&env);
        from.require_auth();

        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::TokenAddress)
            .expect("Not initialized");
        let client = token::Client::new(&env, &token_addr);

        // Transfer from User -> Contract
        client.transfer(&from, &env.current_contract_address(), &amount);

        // (In a real app, we would mint "Pool Share Tokens" here)
        env.events()
            .publish((Symbol::new(&env, "deposit"), from), amount);
    }

    // 3. BORROW: Borrow against an invoice (Simplified)
    pub fn borrow(env: Env, borrower: Address, amount: i128) {
        Self::check_paused(&env);
        borrower.require_auth();

        // 1. Check if the pool has enough funds
        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::TokenAddress)
            .expect("Not initialized");
        let client = token::Client::new(&env, &token_addr);

        let pool_balance = client.balance(&env.current_contract_address());
        if amount > pool_balance {
            panic!("Insufficient pool liquidity");
        }

        // 2. Transfer funds Contract -> Borrower
        client.transfer(&env.current_contract_address(), &borrower, &amount);

        env.events()
            .publish((Symbol::new(&env, "borrow"), borrower), amount);
    }

    // Helper function to calculate interest (5% APY)
    fn calculate_interest(principal: i128, start_time: u64, end_time: u64) -> i128 {
        const YEAR_IN_SECONDS: u64 = 31_536_000; // 365.25 days
        const APY_BPS: u64 = 500; // 5% expressed in basis points

        if end_time <= start_time {
            return 0;
        }

        let duration = end_time - start_time;
        let interest =
            principal * APY_BPS as i128 * duration as i128 / (10_000 * YEAR_IN_SECONDS as i128);
        interest
    }

    // Helper function to extend storage TTL
    fn extend_storage_ttl(env: &Env) {
        // Extend TTL to 535,680 ledgers (approx 30 days)
        env.storage().instance().extend_ttl(535_680, 535_680);
    }

    // SET BACKEND PUBKEY: Initialize backend public key for signature verification
    pub fn set_backend_pubkey(env: Env, pubkey: BytesN<32>) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::BackendPubkey, &pubkey);
        Self::extend_storage_ttl(&env);
    }

    // CREATE LOAN: Create a new loan record
    pub fn create_loan(
        env: Env,
        borrower: Address,
        invoice_id: u64,
        principal: i128,
        due_date: u64,
    ) -> u64 {
        Self::check_paused(&env);
        borrower.require_auth();

        let current_time = env.ledger().timestamp();
        let interest = Self::calculate_interest(principal, current_time, due_date);

        let mut loan_id = env
            .storage()
            .instance()
            .get(&DataKey::LoanId)
            .unwrap_or(0u64);
        loan_id += 1;

        let loan = Loan {
            id: loan_id,
            borrower: borrower.clone(),
            invoice_id,
            principal,
            interest,
            start_time: current_time,
            due_date,
            is_repaid: false,
            is_defaulted: false,
        };

        env.storage().instance().set(&DataKey::Loan(loan_id), &loan);
        env.storage().instance().set(&DataKey::LoanId, &loan_id);
        Self::extend_storage_ttl(&env);

        env.events()
            .publish((Symbol::new(&env, "loan_created"), borrower), loan_id);
        loan_id
    }

    // REPAY LOAN: Repay a loan and unlock collateral
    pub fn repay_loan(env: Env, loan_id: u64) {
        Self::check_paused(&env);

        let mut loan: Loan = env
            .storage()
            .instance()
            .get(&DataKey::Loan(loan_id))
            .expect("Loan not found");

        if loan.is_repaid {
            panic!("Loan already repaid");
        }

        if loan.is_defaulted {
            panic!("Loan defaulted - use liquidation instead");
        }

        loan.borrower.require_auth();

        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::TokenAddress)
            .expect("Not initialized");
        let client = token::Client::new(&env, &token_addr);

        let current_time = env.ledger().timestamp();
        let current_interest =
            Self::calculate_interest(loan.principal, loan.start_time, current_time);
        let total_repayment = loan.principal + current_interest;

        // Check borrower's USDC balance
        let borrower_balance = client.balance(&loan.borrower);
        if borrower_balance < total_repayment {
            panic!("Insufficient USDC balance");
        }

        // Transfer repayment from borrower to contract
        client.transfer(
            &loan.borrower,
            &env.current_contract_address(),
            &total_repayment,
        );

        // Update loan status
        loan.is_repaid = true;
        env.storage().instance().set(&DataKey::Loan(loan_id), &loan);
        Self::extend_storage_ttl(&env);

        // In a real implementation, we would transfer the NFT back to the borrower
        // For now, we just emit an event
        env.events()
            .publish((Symbol::new(&env, "loan_repaid"), loan.borrower), loan_id);
    }

    // LIQUIDATE: Liquidate a defaulted loan
    pub fn liquidate(env: Env, loan_id: u64) {
        Self::check_paused(&env);

        let mut loan: Loan = env
            .storage()
            .instance()
            .get(&DataKey::Loan(loan_id))
            .expect("Loan not found");

        if loan.is_repaid {
            panic!("Cannot liquidate repaid loan");
        }

        if loan.is_defaulted {
            panic!("Loan already liquidated");
        }

        let current_time = env.ledger().timestamp();
        if current_time <= loan.due_date {
            panic!("Cannot liquidate healthy loan");
        }

        let liquidator = env.current_contract_address(); // In real implementation, this would be the caller
        liquidator.require_auth();

        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::TokenAddress)
            .expect("Not initialized");
        let client = token::Client::new(&env, &token_addr);

        // Transfer principal from liquidator to contract
        client.transfer(
            &liquidator,
            &env.current_contract_address(),
            &loan.principal,
        );

        // Update loan status
        loan.is_defaulted = true;
        env.storage().instance().set(&DataKey::Loan(loan_id), &loan);
        Self::extend_storage_ttl(&env);

        // In a real implementation, we would transfer the NFT to the liquidator
        env.events()
            .publish((Symbol::new(&env, "loan_liquidated"), liquidator), loan_id);
    }

    // GET LOAN: Retrieve loan details
    pub fn get_loan(env: Env, loan_id: u64) -> Option<Loan> {
        env.storage().instance().get(&DataKey::Loan(loan_id))
    }

    // 4. VIEW: Check contract balance
    pub fn get_pool_balance(env: Env) -> i128 {
        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::TokenAddress)
            .expect("Not initialized");
        let client = token::Client::new(&env, &token_addr);
        client.balance(&env.current_contract_address())
    }
}
