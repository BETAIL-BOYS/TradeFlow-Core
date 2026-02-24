#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Bytes, BytesN, Env, Symbol};

mod tests;

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
    Invoice(u64),  // Maps ID -> Invoice
    TokenId,       // Tracks the next available ID
    BackendPubkey, // Backend public key for signature verification
}

#[contract]
pub struct InvoiceContract;

#[contractimpl]
impl InvoiceContract {
    // Helper function to extend storage TTL
    fn extend_storage_ttl(env: &Env) {
        // Extend TTL to 535,680 ledgers (approx 30 days)
        env.storage().instance().extend_ttl(535_680, 535_680);
    }

    // Helper function to check admin authorization
    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::BackendPubkey)
            .expect("Backend pubkey not set");
        admin.require_auth();
    }

    // SET BACKEND PUBKEY: Initialize backend public key for signature verification
    pub fn set_backend_pubkey(env: Env, pubkey: BytesN<32>) {
        // For simplicity, we'll allow anyone to set this initially
        // In production, this should be admin-only
        env.storage()
            .instance()
            .set(&DataKey::BackendPubkey, &pubkey);
        Self::extend_storage_ttl(&env);
    }

    // Helper function to verify backend signature
    fn verify_signature(
        env: &Env,
        _user: &Address,
        amount: i128,
        risk_score: u32,
        signature: &BytesN<64>,
    ) -> bool {
        let backend_pubkey: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::BackendPubkey)
            .expect("Backend pubkey not set");

        // Create message payload as bytes: (amount as i128, risk_score as u32)
        let mut message = Bytes::new(&env);

        // Add amount (i128) as 16 bytes little-endian
        let amount_bytes = amount.to_le_bytes();
        for byte in amount_bytes.iter() {
            message.push_back(*byte);
        }

        // Add risk_score (u32) as 4 bytes little-endian
        let risk_bytes = risk_score.to_le_bytes();
        for byte in risk_bytes.iter() {
            message.push_back(*byte);
        }

        env.crypto()
            .ed25519_verify(&backend_pubkey, &message, signature);

        // If ed25519_verify doesn't panic, the signature is valid
        true
    }

    // 1. MINT: Create a new Invoice NFT with signature verification
    pub fn mint(
        env: Env,
        owner: Address,
        amount: i128,
        due_date: u64,
        risk_score: u32,
        signature: BytesN<64>,
    ) -> u64 {
        owner.require_auth(); // Ensure the caller is who they say they are

        // Check if invoice is expired
        let current_timestamp = env.ledger().timestamp();
        if due_date <= current_timestamp {
            panic!("INVOICE_EXPIRED");
        }

        // Verify backend signature
        if !Self::verify_signature(&env, &owner, amount, risk_score, &signature) {
            panic!("INVALID_SIGNATURE");
        }

        // Get the current ID count
        let mut current_id = env
            .storage()
            .instance()
            .get(&DataKey::TokenId)
            .unwrap_or(0u64);
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
        env.storage()
            .instance()
            .set(&DataKey::Invoice(current_id), &invoice);
        env.storage().instance().set(&DataKey::TokenId, &current_id);
        Self::extend_storage_ttl(&env);

        // Emit an event (so our API can see it later)
        env.events()
            .publish((Symbol::new(&env, "mint"), owner), current_id);

        current_id
    }

    // 2. GET: Read invoice details
    /// Get invoice details by ID (read-only view function)
    ///
    /// Panics with "InvoiceNotFound" if the ID does not exist
    pub fn get_invoice(env: Env, id: u64) -> Invoice {
        env.storage()
            .instance()
            .get(&DataKey::Invoice(id))
            .unwrap_or_else(|| panic!("InvoiceNotFound"))
    }

    // 3. REPAY: Mark the invoice as paid
    pub fn repay(env: Env, id: u64) {
        let mut invoice: Invoice = env
            .storage()
            .instance()
            .get(&DataKey::Invoice(id))
            .expect("Invoice not found");

        invoice.owner.require_auth(); // Only the owner can repay

        // (In a real app, we would transfer USDC here. For MVP, we just flip the switch.)
        invoice.is_repaid = true;

        env.storage()
            .instance()
            .set(&DataKey::Invoice(id), &invoice);
        Self::extend_storage_ttl(&env);

        env.events()
            .publish((Symbol::new(&env, "repay"), invoice.owner), id);
    }
}
