#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol};

#[contracttype]
#[derive(Clone)]
pub struct Invoice {
    pub id: u64,
    pub owner: Address,
    pub amount: i128,
    pub due_date: u64,
    pub is_repaid: bool,
}

#[contracttype]
pub enum DataKey {
    Invoice(u64), // Maps ID -> Invoice
    TokenId,      // Tracks the next available ID
}

#[contract]
pub struct InvoiceContract;

#[contractimpl]
impl InvoiceContract {
    // 1. MINT: Create a new Invoice NFT
    pub fn mint(env: Env, owner: Address, amount: i128, due_date: u64) -> u64 {
        owner.require_auth(); // Ensure the caller is who they say they are

        // Get the current ID count
        let mut current_id = env.storage().instance().get(&DataKey::TokenId).unwrap_or(0u64);
        current_id += 1;

        // Create the invoice object
        let invoice = Invoice {
            id: current_id,
            owner: owner.clone(),
            amount,
            due_date,
            is_repaid: false,
        };

        // Save to storage
        env.storage().instance().set(&DataKey::Invoice(current_id), &invoice);
        env.storage().instance().set(&DataKey::TokenId, &current_id);

        // Emit an event (so our API can see it later)
        env.events().publish((Symbol::new(&env, "mint"), owner), current_id);

        current_id
    }

    // 2. GET: Read invoice details
    pub fn get_invoice(env: Env, id: u64) -> Option<Invoice> {
        env.storage().instance().get(&DataKey::Invoice(id))
    }

    // 3. REPAY: Mark the invoice as paid
    pub fn repay(env: Env, id: u64) {
        let mut invoice: Invoice = env.storage().instance().get(&DataKey::Invoice(id)).expect("Invoice not found");
        
        invoice.owner.require_auth(); // Only the owner can repay

        // (In a real app, we would transfer USDC here. For MVP, we just flip the switch.)
        invoice.is_repaid = true;

        env.storage().instance().set(&DataKey::Invoice(id), &invoice);
        
        env.events().publish((Symbol::new(&env, "repay"), invoice.owner), id);
    }
}