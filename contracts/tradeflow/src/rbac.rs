use soroban_sdk::{symbol_short, Address, Bytes, BytesN, Env};
use crate::DataKey;

// Bump instance TTL when it falls below ~30 days (at 5 s/ledger).
pub const ROLE_TTL_THRESHOLD: u32 = 518_400;
// Extend to ~100 days when bumped.
pub const ROLE_TTL_EXTEND_TO: u32 = 1_728_000;

// ---------------------------------------------------------------------------
// Role constants — computed as SHA-256 of the role name string.
// ---------------------------------------------------------------------------

pub fn default_admin_role(env: &Env) -> BytesN<32> {
    env.crypto().sha256(&Bytes::from_slice(env, b"DEFAULT_ADMIN_ROLE")).into()
}

pub fn fee_manager_role(env: &Env) -> BytesN<32> {
    env.crypto().sha256(&Bytes::from_slice(env, b"FEE_MANAGER_ROLE")).into()
}

pub fn pauser_role(env: &Env) -> BytesN<32> {
    env.crypto().sha256(&Bytes::from_slice(env, b"PAUSER_ROLE")).into()
}

pub fn kyc_manager_role(env: &Env) -> BytesN<32> {
    env.crypto().sha256(&Bytes::from_slice(env, b"KYC_MANAGER_ROLE")).into()
}

// ---------------------------------------------------------------------------
// Core helpers
// ---------------------------------------------------------------------------

/// Returns true if `account` currently holds `role`.
pub fn has_role(env: &Env, role: &BytesN<32>, account: &Address) -> bool {
    let key = DataKey::Role(account.clone(), role.clone());
    env.storage()
        .instance()
        .get::<_, bool>(&key)
        .unwrap_or(false)
}

/// Asserts that `account` is authenticated *and* holds `role`.
/// Panics with a descriptive message on either failure.
pub fn require_role(env: &Env, role: &BytesN<32>, account: &Address) {
    account.require_auth();
    if !has_role(env, role, account) {
        panic!("AccessControl: caller is missing required role");
    }
}

// ---------------------------------------------------------------------------
// Internal mutation helpers (no auth check — callers are responsible).
// ---------------------------------------------------------------------------

/// Writes the `(account, role) => true` mapping and bumps instance TTL.
pub fn grant_role_unchecked(env: &Env, role: BytesN<32>, account: Address) {
    let key = DataKey::Role(account.clone(), role.clone());
    env.storage().instance().set(&key, &true);
    env.storage()
        .instance()
        .extend_ttl(ROLE_TTL_THRESHOLD, ROLE_TTL_EXTEND_TO);

    env.events()
        .publish((symbol_short!("RoleGrant"), role), account);
}

/// Removes the `(account, role)` mapping from instance storage.
pub fn revoke_role_unchecked(env: &Env, role: BytesN<32>, account: Address) {
    let key = DataKey::Role(account.clone(), role.clone());
    env.storage().instance().remove(&key);

    env.events()
        .publish((symbol_short!("RlRevoke"), role), account);
}
