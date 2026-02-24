use crate::LendingPool;
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    ContractPaused = 2,
    InsufficientLiquidity = 3,
    LoanNotFound = 4,
    LoanAlreadyRepaid = 5,
    LoanDefaulted = 6,
    InsufficientBalance = 7,
    CannotLiquidateHealthyLoan = 8,
    Unauthorized = 9,
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::contractclient::LendingPoolClient;
    use soroban_sdk::{testutils::Address as TestAddress, testutils::Bytes as TestBytes};

    #[test]
    fn test_initialization() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let token_address = Address::generate(&env);

        client.init(&admin, &token_address);

        assert!(!client.is_paused());
    }

    #[test]
    #[should_panic(expected = "Already initialized")]
    fn test_double_initialization() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let token_address = Address::generate(&env);

        client.init(&admin, &token_address);
        client.init(&admin, &token_address);
    }

    #[test]
    fn test_pause_functionality() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let token_address = Address::generate(&env);
        client.init(&admin, &token_address);

        // Test pausing
        client.set_paused(&true);
        assert!(client.is_paused());

        // Test unpausing
        client.set_paused(&false);
        assert!(!client.is_paused());
    }

    #[test]
    #[should_panic(expected = "CONTRACT_PAUSED")]
    fn test_deposit_when_paused() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let token_address = Address::generate(&env);
        client.init(&admin, &token_address);

        client.set_paused(&true);

        let user = Address::generate(&env);
        client.deposit(&user, &1000);
    }

    #[test]
    #[should_panic(expected = "CONTRACT_PAUSED")]
    fn test_borrow_when_paused() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let token_address = Address::generate(&env);
        client.init(&admin, &token_address);

        client.set_paused(&true);

        let borrower = Address::generate(&env);
        client.borrow(&borrower, &1000);
    }

    #[test]
    fn test_create_loan() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let token_address = Address::generate(&env);
        client.init(&admin, &token_address);

        let borrower = Address::generate(&env);
        let due_date = env.ledger().timestamp() + 86400;
        let loan_id = client.create_loan(&borrower, &1, &1000, &due_date);

        let loan = client.get_loan(&loan_id).unwrap();
        assert_eq!(loan.borrower, borrower);
        assert_eq!(loan.principal, 1000);
        assert_eq!(loan.invoice_id, 1);
        assert!(!loan.is_repaid);
        assert!(!loan.is_defaulted);
    }

    #[test]
    fn test_repay_loan() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let token_address = Address::generate(&env);
        client.init(&admin, &token_address);

        let borrower = Address::generate(&env);
        let due_date = env.ledger().timestamp() + 86400;
        let loan_id = client.create_loan(&borrower, &1, &1000, &due_date);

        // In a real test, we would set up the token contract and balance
        // For now, we'll just test the logic
        client.repay_loan(&loan_id);

        let loan = client.get_loan(&loan_id).unwrap();
        assert!(loan.is_repaid);
    }

    #[test]
    #[should_panic(expected = "Loan already repaid")]
    fn test_repay_already_repaid_loan() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let token_address = Address::generate(&env);
        client.init(&admin, &token_address);

        let borrower = Address::generate(&env);
        let due_date = env.ledger().timestamp() + 86400;
        let loan_id = client.create_loan(&borrower, &1, &1000, &due_date);

        client.repay_loan(&loan_id);
        client.repay_loan(&loan_id);
    }

    #[test]
    fn test_liquidate_loan() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let token_address = Address::generate(&env);
        client.init(&admin, &token_address);

        let borrower = Address::generate(&env);
        let past_date = env.ledger().timestamp() - 86400; // Past due date
        let loan_id = client.create_loan(&borrower, &1, &1000, &past_date);

        client.liquidate(&loan_id);

        let loan = client.get_loan(&loan_id).unwrap();
        assert!(loan.is_defaulted);
    }

    #[test]
    #[should_panic(expected = "Cannot liquidate healthy loan")]
    fn test_liquidate_healthy_loan() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let token_address = Address::generate(&env);
        client.init(&admin, &token_address);

        let borrower = Address::generate(&env);
        let future_date = env.ledger().timestamp() + 86400; // Future due date
        let loan_id = client.create_loan(&borrower, &1, &1000, &future_date);

        client.liquidate(&loan_id);
    }

    #[test]
    fn test_interest_calculation() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let token_address = Address::generate(&env);
        client.init(&admin, &token_address);

        let borrower = Address::generate(&env);
        let one_year_later = env.ledger().timestamp() + 31_536_000; // 1 year
        let loan_id = client.create_loan(&borrower, &1, &1000, &one_year_later);

        let loan = client.get_loan(&loan_id).unwrap();
        // 5% of 1000 = 50 interest for 1 year
        assert_eq!(loan.interest, 50);
    }
}
