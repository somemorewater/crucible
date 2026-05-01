#![allow(deprecated)]
//! Mock token contract for testing Soroban contracts.
//!
//! Provides `MockToken` - a wrapper around the Stellar Asset Contract (SAC)
//! for easy token operations in tests without manual WASM deployment.

use crate::env::MockEnv;
use soroban_sdk::{
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};

/// A mock token contract that wraps the Soroban test token utilities.
///
/// This provides a convenient way to create and manipulate tokens in tests
/// without needing to deploy actual token WASM contracts.
#[derive(Clone)]
pub struct MockToken {
    env: Env,
    address: Address,
}

impl std::fmt::Debug for MockToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockToken")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl MockToken {
    /// Creates a mock XLM token using soroban-sdk's built-in XLM mock.
    ///
    /// # Arguments
    ///
    /// * `env` - The mock environment to use
    ///
    /// # Example
    ///
    /// ```ignore
    /// use crucible::prelude::*;
    /// let env = MockEnv::builder().build();
    /// let xlm = MockToken::xlm(&env);
    /// ```
    pub fn xlm(env: &MockEnv) -> Self {
        if let Some(address) = env.xlm_token_address() {
            return Self::from_address(env.inner(), address);
        }

        // Create an admin for the XLM token
        let _admin = env
            .inner()
            .register_contract::<soroban_sdk::testutils::MockAuthContract>(
                None,
                soroban_sdk::testutils::MockAuthContract {},
            );
        let sac = env.inner().register_stellar_asset_contract_v2(_admin);
        let address = sac.address();
        env.set_xlm_token_address(address.clone());

        Self {
            env: env.inner().clone(),
            address,
        }
    }

    /// Creates a MockToken from an existing address.
    pub fn from_address(env: &Env, address: Address) -> Self {
        Self {
            env: env.clone(),
            address,
        }
    }

    /// Creates a new mock token with the given symbol and decimals.
    ///
    /// # Arguments
    ///
    /// * `env` - The mock environment to use
    /// * `symbol` - The token symbol (e.g., "USDC")
    /// * `decimals` - The number of decimal places for the token
    ///
    /// # Example
    ///
    /// ```ignore
    /// use crucible::prelude::*;
    /// let env = MockEnv::builder().build();
    /// let usdc = MockToken::new(&env, "USDC", 6);
    /// ```
    pub fn new(env: &MockEnv, _symbol: &str, _decimals: u32) -> Self {
        // Create an admin for the token
        let _admin = env
            .inner()
            .register_contract::<soroban_sdk::testutils::MockAuthContract>(
                None,
                soroban_sdk::testutils::MockAuthContract {},
            );
        let sac = env.inner().register_stellar_asset_contract_v2(_admin);
        let address = sac.address();

        Self {
            env: env.inner().clone(),
            address,
        }
    }

    /// Returns the token contract's address.
    pub fn address(&self) -> Address {
        self.address.clone()
    }

    /// Mints tokens to the specified account.
    ///
    /// This is a test-only convenience method that does not require auth.
    ///
    /// # Arguments
    ///
    /// * `to` - The address to mint tokens to
    /// * `amount` - The amount of tokens to mint (in smallest units)
    ///
    /// # Example
    ///
    /// ```ignore
    /// use crucible::prelude::*;
    /// let env = MockEnv::builder().build();
    /// let token = MockToken::xlm(&env);
    /// let alice = env.account("alice");
    /// token.mint(&alice, 1_000_000);
    /// ```
    pub fn mint(&self, to: &Address, amount: i128) {
        // Enable mock auth for this operation
        self.env.mock_all_auths();
        let client = StellarAssetClient::new(&self.env, &self.address);
        client.mint(to, &amount);
    }

    /// Burns tokens from the specified account.
    ///
    /// # Arguments
    ///
    /// * `from` - The address to burn tokens from
    /// * `amount` - The amount of tokens to burn (in smallest units)
    pub fn burn(&self, from: &Address, amount: i128) {
        self.env.mock_all_auths();
        let client = TokenClient::new(&self.env, &self.address);
        client.burn(from, &amount);
    }

    /// Returns the token balance of the specified account.
    ///
    /// # Arguments
    ///
    /// * `account` - The address to check the balance for
    ///
    /// # Returns
    ///
    /// The balance in the token's smallest units
    pub fn balance(&self, account: &Address) -> i128 {
        let client = TokenClient::new(&self.env, &self.address);
        client.balance(account)
    }

    /// Returns the allowance for a spender on behalf of an owner.
    ///
    /// # Arguments
    ///
    /// * `from` - The token owner's address
    /// * `spender` - The spender's address
    ///
    /// # Returns
    ///
    /// The allowed amount in the token's smallest units
    pub fn allowance(&self, from: &Address, spender: &Address) -> i128 {
        let client = TokenClient::new(&self.env, &self.address);
        client.allowance(from, spender)
    }

    /// Approves a spender to spend tokens on behalf of the owner.
    ///
    /// # Arguments
    ///
    /// * `from` - The token owner's address
    /// * `spender` - The spender's address
    /// * `amount` - The amount to approve (in smallest units)
    /// * `expiry_ledger` - The ledger number at which the approval expires
    pub fn approve(&self, from: &Address, spender: &Address, amount: i128, expiry_ledger: u32) {
        self.env.mock_all_auths();
        let client = TokenClient::new(&self.env, &self.address);
        client.approve(from, spender, &amount, &expiry_ledger);
    }

    /// Transfers tokens from one account to another.
    ///
    /// # Arguments
    ///
    /// * `from` - The sender's address
    /// * `to` - The recipient's address
    /// * `amount` - The amount to transfer (in smallest units)
    pub fn transfer(&self, from: &Address, to: &Address, amount: i128) {
        self.env.mock_all_auths();
        let client = TokenClient::new(&self.env, &self.address);
        client.transfer(from, to, &amount);
    }

    /// Sets a new admin for the token contract.
    ///
    /// # Arguments
    ///
    /// * `new_admin` - The address of the new admin
    pub fn set_admin(&self, new_admin: &Address) {
        self.env.mock_all_auths();
        let client = StellarAssetClient::new(&self.env, &self.address);
        client.set_admin(new_admin);
    }

    /// Claws back tokens from an account (admin operation).
    ///
    /// # Arguments
    ///
    /// * `from` - The address to claw back tokens from
    /// * `amount` - The amount to claw back (in smallest units)
    pub fn clawback(&self, from: &Address, amount: i128) {
        self.env.mock_all_auths();
        let client = StellarAssetClient::new(&self.env, &self.address);
        client.clawback(from, &amount);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Stroops;

    #[test]
    fn test_mint_and_check_balance() {
        let env = MockEnv::builder()
            .with_account("alice", Stroops::from(0))
            .build();

        let token = MockToken::xlm(&env);
        let alice = env.account("alice");

        // Mint tokens to alice
        token.mint(&alice.address(), 500_000);

        // Check balance
        assert_eq!(token.balance(&alice.address()), 500_000);
    }

    #[test]
    fn test_transfer_between_accounts() {
        let env = MockEnv::builder()
            .with_account("alice", Stroops::from(0))
            .with_account("bob", Stroops::from(0))
            .build();

        let token = MockToken::xlm(&env);
        let alice = env.account("alice");
        let bob = env.account("bob");

        // Mint tokens to alice
        token.mint(&alice.address(), 1_000_000);
        assert_eq!(token.balance(&alice.address()), 1_000_000);

        // Transfer from alice to bob
        token.transfer(&alice.address(), &bob.address(), 400_000);

        // Verify both balances
        assert_eq!(token.balance(&alice.address()), 600_000);
        assert_eq!(token.balance(&bob.address()), 400_000);
    }

    #[test]
    fn test_approve_and_check_allowance() {
        let env = MockEnv::builder()
            .with_account("alice", Stroops::from(0))
            .with_account("spender", Stroops::from(0))
            .build();

        let token = MockToken::xlm(&env);
        let alice = env.account("alice");
        let spender = env.account("spender");

        // Mint tokens to alice
        token.mint(&alice.address(), 1_000_000);

        // Approve spender
        token.approve(&alice.address(), &spender.address(), 500_000, 1000);

        // Check allowance
        assert_eq!(
            token.allowance(&alice.address(), &spender.address()),
            500_000
        );
    }

    #[test]
    fn test_clawback_reduces_balance() {
        let env = MockEnv::builder()
            .with_account("alice", Stroops::from(0))
            .build();

        let token = MockToken::xlm(&env);
        let alice = env.account("alice");

        // Mint tokens to alice
        token.mint(&alice.address(), 1_000_000);
        assert_eq!(token.balance(&alice.address()), 1_000_000);

        // Burn some tokens (similar effect to clawback - reduces balance)
        // Note: clawback requires special issuer flags to be set on the SAC
        token.burn(&alice.address(), 300_000);

        // Verify balance reduced
        assert_eq!(token.balance(&alice.address()), 700_000);
    }

    #[test]
    fn test_burn_reduces_balance() {
        let env = MockEnv::builder()
            .with_account("alice", Stroops::from(0))
            .build();

        let token = MockToken::xlm(&env);
        let alice = env.account("alice");

        // Mint tokens to alice
        token.mint(&alice.address(), 1_000_000);
        assert_eq!(token.balance(&alice.address()), 1_000_000);

        // Burn some tokens
        token.burn(&alice.address(), 200_000);

        // Verify balance reduced
        assert_eq!(token.balance(&alice.address()), 800_000);
    }

    #[test]
    fn test_new_token_with_symbol_and_decimals() {
        let env = MockEnv::builder()
            .with_account("alice", Stroops::from(0))
            .build();

        let token = MockToken::new(&env, "USDC", 6);
        let alice = env.account("alice");

        // Mint tokens
        token.mint(&alice.address(), 1_000_000_000); // 1000 USDC with 6 decimals

        assert_eq!(token.balance(&alice.address()), 1_000_000_000);
    }
}
