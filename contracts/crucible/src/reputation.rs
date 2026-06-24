use soroban_sdk::testutils::ContractFunctionSet;
use soroban_sdk::{contracttype, symbol_short, Address, Env, Val};

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Reputation(Address),
}

pub struct ReputationContract {
    // We don't need to store anything in the struct because we use the contract's instance storage.
    // But we need to satisfy the ContractFunctionSet trait.
}

impl ReputationContract {
    pub fn new() -> Self {
        Self {}
    }

    fn initialize(&self, env: Env, admin: Address) {
        // Check if already initialized
        let existing_admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        if existing_admin.is_some() {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.events()
            .publish((symbol_short!("initialized"), admin), 0u32);
    }

    fn set_reputation(&self, env: Env, caller: Address, account: Address, score: i32) {
        caller.require_auth();
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert_eq!(caller, admin, "not admin");
        env.storage()
            .instance()
            .set(&DataKey::Reputation(account), &score);
        env.events()
            .publish((symbol_short!("reputation_set"), account), score);
    }

    fn increase_reputation(&self, env: Env, caller: Address, account: Address, amount: i32) {
        caller.require_auth();
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert_eq!(caller, admin, "not admin");
        let current: i32 = env
            .storage()
            .instance()
            .get(&DataKey::Reputation(account.clone()))
            .unwrap_or(0);
        let new_score = current + amount;
        env.storage()
            .instance()
            .set(&DataKey::Reputation(account), &new_score);
        env.events()
            .publish((symbol_short!("reputation_increased"), account), amount);
    }

    fn decrease_reputation(&self, env: Env, caller: Address, account: Address, amount: i32) {
        caller.require_auth();
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert_eq!(caller, admin, "not admin");
        let current: i32 = env
            .storage()
            .instance()
            .get(&DataKey::Reputation(account.clone()))
            .unwrap_or(0);
        let new_score = current - amount;
        env.storage()
            .instance()
            .set(&DataKey::Reputation(account), &new_score);
        env.events()
            .publish((symbol_short!("reputation_decreased"), account), amount);
    }

    fn get_reputation(&self, env: Env, account: Address) -> i32 {
        env.storage()
            .instance()
            .get(&DataKey::Reputation(account))
            .unwrap_or(0)
    }
}

impl ContractFunctionSet for ReputationContract {
    fn call(&self, func: &str, env: Env, args: &[Val]) -> Option<Val> {
        match func {
            "initialize" => {
                let admin = args.get(0)?.clone().try_into().ok()?;
                self.initialize(env, admin);
                Some(Val::Void)
            }
            "set_reputation" => {
                let caller = args.get(0)?.clone().try_into().ok()?;
                let account = args.get(1)?.clone().try_into().ok()?;
                let score = args.get(2)?.clone().try_into().ok()?;
                self.set_reputation(env, caller, account, score);
                Some(Val::Void)
            }
            "increase_reputation" => {
                let caller = args.get(0)?.clone().try_into().ok()?;
                let account = args.get(1)?.clone().try_into().ok()?;
                let amount = args.get(2)?.clone().try_into().ok()?;
                self.increase_reputation(env, caller, account, amount);
                Some(Val::Void)
            }
            "decrease_reputation" => {
                let caller = args.get(0)?.clone().try_into().ok()?;
                let account = args.get(1)?.clone().try_into().ok()?;
                let amount = args.get(2)?.clone().try_into().ok()?;
                self.decrease_reputation(env, caller, account, amount);
                Some(Val::Void)
            }
            "get_reputation" => {
                let account = args.get(0)?.clone().try_into().ok()?;
                let score = self.get_reputation(env, account);
                Some(Val::from(score))
            }
            _ => None,
        }
    }
}


// We'll implement a client struct similar to MockToken for ease of use.
#[derive(Clone)]
pub struct ReputationContractClient {
    env: Env,
    address: Address,
}

impl ReputationContractClient {
    pub fn new(env: &Env, address: &Address) -> Self {
        Self {
            env: env.clone(),
            address: address.clone(),
        }
    }

    pub fn address(&self) -> &Address {
        &self.address
    }

    /// Initialize the reputation contract with an admin address.
    /// This should be called by the deployer.
    pub fn initialize(&self, admin: &Address) {
        self.env.mock_all_auths();
        let client = soroban_sdk::contractclient::ContractClient::new(&self.env, &self.address);
        client.call(&symbol_short!("initialize"), &(admin,));
    }

    /// Set the reputation of an account to a specific score. Admin only.
    pub fn set_reputation(&self, admin: &Address, account: &Address, score: i32) {
        self.env.mock_all_auths();
        let client = soroban_sdk::contractclient::ContractClient::new(&self.env, &self.address);
        client.call(&symbol_short!("set_reputation"), &(admin, account, score));
    }

    /// Increase the reputation of an account by a given amount. Admin only.
    pub fn increase_reputation(&self, admin: &Address, account: &Address, amount: i32) {
        self.env.mock_all_auths();
        let client = soroban_sdk::contractclient::ContractClient::new(&self.env, &self.address);
        client.call(
            &symbol_short!("increase_reputation"),
            &(admin, account, amount),
        );
    }

    /// Decrease the reputation of an account by a given amount. Admin only.
    pub fn decrease_reputation(&self, admin: &Address, account: &Address, amount: i32) {
        self.env.mock_all_auths();
        let client = soroban_sdk::contractclient::ContractClient::new(&self.env, &self.address);
        client.call(
            &symbol_short!("decrease_reputation"),
            &(admin, account, amount),
        );
    }

    /// Get the reputation of an account.
    pub fn get_reputation(&self, account: &Address) -> i32 {
        let client = soroban_sdk::contractclient::ContractClient::new(&self.env, &self.address);
        client
            .call(&symbol_short!("get_reputation"), &(account,))
            .unwrap()
            .try_into()
            .unwrap()
    }

    /// Try to increase the reputation of an account by a given amount. Returns Ok(()) if successful, Err(()) if failed.
    pub fn try_increase_reputation(
        &self,
        admin: &Address,
        account: &Address,
        amount: i32,
    ) -> Result<(), ()> {
        let client = soroban_sdk::contractclient::ContractClient::new(&self.env, &self.address);
        match client.try_call(
            &symbol_short!("increase_reputation"),
            &(admin, account, amount),
        ) {
            Ok(_) => Ok(()),
            Err(_) => Err(()),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::env::{MockEnv, Stroops};

    #[test]
    fn test_reputation_contract() {
        // Create accounts before lookup — fixes issue #493
        let env = MockEnv::builder()
            .with_account("admin", Stroops::xlm(100))
            .with_account("user", Stroops::xlm(100))
            .build();

        let admin = env.account("admin");
        let user = env.account("user");

        // Deploy the reputation contract via the inner Soroban env
        let address = env.inner().register_contract(None, ReputationContract::new());
        let client = ReputationContractClient::new(env.inner(), &address);

        // Initialize with admin
        client.initialize(&admin.address());

        // Set reputation for user
        client.set_reputation(&admin.address(), &user.address(), 100);
        assert_eq!(client.get_reputation(&user.address()), 100);

        // Increase reputation
        client.increase_reputation(&admin.address(), &user.address(), 50);
        assert_eq!(client.get_reputation(&user.address()), 150);

        // Decrease reputation
        client.decrease_reputation(&admin.address(), &user.address(), 30);
        assert_eq!(client.get_reputation(&user.address()), 120);
    }

    #[test]
    fn test_reputation_non_admin_cannot_set() {
        let env = MockEnv::builder()
            .with_account("admin", Stroops::xlm(100))
            .with_account("user", Stroops::xlm(100))
            .build();

        let admin = env.account("admin");
        let user = env.account("user");

        let address = env.inner().register_contract(None, ReputationContract::new());
        let client = ReputationContractClient::new(env.inner(), &address);

        client.initialize(&admin.address());

        // Non-admin should fail: try_increase_reputation returns Err when caller != admin
        let result = client.try_increase_reputation(&user.address(), &user.address(), 10);
        assert!(result.is_err(), "non-admin should not be able to increase reputation");

        // Reputation should remain at default 0
        assert_eq!(client.get_reputation(&user.address()), 0);
    }
}
