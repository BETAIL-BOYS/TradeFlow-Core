use crate::InvoiceContract;
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    InvoiceNotFound = 1,
    InvoiceExpired = 2,
    InvalidSignature = 3,
    AlreadyRepaid = 4,
    Unauthorized = 5,
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::contractclient::InvoiceContractClient;
    use soroban_sdk::{testutils::Address as TestAddress, testutils::Bytes as TestBytes, Bytes};

    #[test]
    fn test_mint_invoice_success() {
        let env = Env::default();
        let contract_id = env.register_contract(None, InvoiceContract);
        let client = InvoiceContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let backend_pubkey = [1u8; 32];
        client.set_backend_pubkey(&backend_pubkey);

        // Create a valid signature (mock)
        let signature = [2u8; 64];

        let due_date = env.ledger().timestamp() + 86400; // Tomorrow
        let invoice_id = client.mint(&owner, &1000, &due_date, &750, &signature);

        let invoice = client.get_invoice(&invoice_id).unwrap();
        assert_eq!(invoice.owner, owner);
        assert_eq!(invoice.amount, 1000);
        assert_eq!(invoice.due_date, due_date);
        assert!(!invoice.is_repaid);
    }

    #[test]
    #[should_panic(expected = "INVOICE_EXPIRED")]
    fn test_mint_expired_invoice() {
        let env = Env::default();
        let contract_id = env.register_contract(None, InvoiceContract);
        let client = InvoiceContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let backend_pubkey = [1u8; 32];
        client.set_backend_pubkey(&backend_pubkey);

        let signature = [2u8; 64];
        let past_date = env.ledger().timestamp() - 86400; // Yesterday

        client.mint(&owner, &1000, &past_date, &750, &signature);
    }

    #[test]
    #[should_panic(expected = "INVALID_SIGNATURE")]
    fn test_mint_invalid_signature() {
        let env = Env::default();
        let contract_id = env.register_contract(None, InvoiceContract);
        let client = InvoiceContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let backend_pubkey = [1u8; 32];
        client.set_backend_pubkey(&backend_pubkey);

        let invalid_signature = [99u8; 64]; // Invalid signature
        let due_date = env.ledger().timestamp() + 86400;

        client.mint(&owner, &1000, &due_date, &750, &invalid_signature);
    }

    #[test]
    fn test_repay_invoice() {
        let env = Env::default();
        let contract_id = env.register_contract(None, InvoiceContract);
        let client = InvoiceContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let backend_pubkey = [1u8; 32];
        client.set_backend_pubkey(&backend_pubkey);

        let signature = [2u8; 64];
        let due_date = env.ledger().timestamp() + 86400;
        let invoice_id = client.mint(&owner, &1000, &due_date, &750, &signature);

        client.repay(&invoice_id);

        let invoice = client.get_invoice(&invoice_id).unwrap();
        assert!(invoice.is_repaid);
    }

    #[test]
    #[should_panic(expected = "Invoice not found")]
    fn test_repay_nonexistent_invoice() {
        let env = Env::default();
        let contract_id = env.register_contract(None, InvoiceContract);
        let client = InvoiceContractClient::new(&env, &contract_id);

        client.repay(&999);
    }
}
