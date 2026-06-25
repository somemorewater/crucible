use soroban_sdk::testutils::ContractFunctionSet;
use soroban_sdk::{contracttype, Address, Env, IntoVal, Symbol, TryFromVal, Val};

fn void_val(env: &Env) -> Val {
    ().into_val(env)
}

fn sym(env: &Env, name: &str) -> Symbol {
    Symbol::new(env, name)
}

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
            .publish((sym(&env, "initialized"), admin), 0u32);
    }

    fn set_reputation(&self, env: Env, caller: Address, account: Address, score: i32) {
        caller.require_auth();
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert_eq!(caller, admin, "not admin");
        env.storage()
            .instance()
            .set(&DataKey::Reputation(account.clone()), &score);
        env.events()
            .publish((sym(&env, "reputation_set"), account), score);
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
            .set(&DataKey::Reputation(account.clone()), &new_score);
        env.events()
            .publish((sym(&env, "reputation_increased"), account), amount);
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
            .set(&DataKey::Reputation(account.clone()), &new_score);
        env.events()
            .publish((sym(&env, "reputation_decreased"), account), amount);
    }

    fn get_reputation(&self, env: Env, account: Address) -> i32 {
        env.storage()
            .instance()
            .get(&DataKey::Reputation(account))
            .unwrap_or(0)
    }
}

impl Default for ReputationContract {
    fn default() -> Self {
        Self::new()
    }
}

impl ContractFunctionSet for ReputationContract {
    fn call(&self, func: &str, env: Env, args: &[Val]) -> Option<Val> {
        match func {
            "initialize" => {
                let admin = Address::try_from_val(&env, args.get(0)?).ok()?;
                self.initialize(env.clone(), admin);
                Some(void_val(&env))
            }
            "set_reputation" => {
                let caller = Address::try_from_val(&env, args.get(0)?).ok()?;
                let account = Address::try_from_val(&env, args.get(1)?).ok()?;
                let score = i32::try_from_val(&env, args.get(2)?).ok()?;
                self.set_reputation(env.clone(), caller, account, score);
                Some(void_val(&env))
            }
            "increase_reputation" => {
                let caller = Address::try_from_val(&env, args.get(0)?).ok()?;
                let account = Address::try_from_val(&env, args.get(1)?).ok()?;
                let amount = i32::try_from_val(&env, args.get(2)?).ok()?;
                self.increase_reputation(env.clone(), caller, account, amount);
                Some(void_val(&env))
            }
            "decrease_reputation" => {
                let caller = Address::try_from_val(&env, args.get(0)?).ok()?;
                let account = Address::try_from_val(&env, args.get(1)?).ok()?;
                let amount = i32::try_from_val(&env, args.get(2)?).ok()?;
                self.decrease_reputation(env.clone(), caller, account, amount);
                Some(void_val(&env))
            }
            "get_reputation" => {
                let account = Address::try_from_val(&env, args.get(0)?).ok()?;
                let score = self.get_reputation(env.clone(), account);
                Some(score.into_val(&env))
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
        self.env.invoke_contract::<()>(
            &self.address,
            &sym(&self.env, "initialize"),
            (admin,).into_val(&self.env),
        );
    }

    /// Set the reputation of an account to a specific score. Admin only.
    pub fn set_reputation(&self, admin: &Address, account: &Address, score: i32) {
        self.env.invoke_contract::<()>(
            &self.address,
            &sym(&self.env, "set_reputation"),
            (admin, account, score).into_val(&self.env),
        );
    }

    /// Increase the reputation of an account by a given amount. Admin only.
    pub fn increase_reputation(&self, admin: &Address, account: &Address, amount: i32) {
        self.env.invoke_contract::<()>(
            &self.address,
            &sym(&self.env, "increase_reputation"),
            (admin, account, amount).into_val(&self.env),
        );
    }

    /// Decrease the reputation of an account by a given amount. Admin only.
    pub fn decrease_reputation(&self, admin: &Address, account: &Address, amount: i32) {
        self.env.invoke_contract::<()>(
            &self.address,
            &sym(&self.env, "decrease_reputation"),
            (admin, account, amount).into_val(&self.env),
        );
    }

    /// Get the reputation of an account.
    pub fn get_reputation(&self, account: &Address) -> i32 {
        self.env.invoke_contract(
            &self.address,
            &sym(&self.env, "get_reputation"),
            (account,).into_val(&self.env),
        )
    }

    /// Try to increase the reputation of an account by a given amount. Returns Ok(()) if successful, Err(()) if failed.
    pub fn try_increase_reputation(
        &self,
        admin: &Address,
        account: &Address,
        amount: i32,
    ) -> Result<(), ()> {
        match self.env.try_invoke_contract::<(), soroban_sdk::Error>(
            &self.address,
            &sym(&self.env, "increase_reputation"),
            (admin, account, amount).into_val(&self.env),
        ) {
            Ok(Ok(())) => Ok(()),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
impl ReputationContractClient {
    /// Test-only helper that mocks all authorizations before running `f`.
    ///
    /// Use this in tests that exercise happy-path contract behavior. For
    /// authorization tests, prefer `MockEnv::mock_auths` with specific entries
    /// so missing or invalid auth is not masked.
    pub fn with_mock_all_auths<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        self.env.mock_all_auths();
        f(self)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::assert_reverts;
    use crate::env::MockEnv;
    use crate::prelude::Stroops;
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke, Register};
    use soroban_sdk::IntoVal;

    struct Fixture {
        env: MockEnv,
        contract_id: Address,
        client: ReputationContractClient,
        admin: crate::account::AccountHandle,
        user: crate::account::AccountHandle,
    }

    impl Fixture {
        fn setup() -> Self {
            let env = MockEnv::builder()
                .with_account("admin", Stroops::from(0))
                .with_account("user", Stroops::from(0))
                .build();
            let admin = env.account("admin");
            let user = env.account("user");
            let contract_id = ReputationContract::new().register(env.inner(), None, ());
            let client = ReputationContractClient::new(env.inner(), &contract_id);

            // initialize does not require auth; avoid leaving mock_all_auths enabled
            client.initialize(&admin.address());

            Self {
                env,
                contract_id,
                client,
                admin,
                user,
            }
        }

        fn mock_auth_for(
            &self,
            caller: &crate::account::AccountHandle,
            fn_name: &str,
            args: impl IntoVal<Env, soroban_sdk::Vec<Val>>,
        ) {
            self.env.inner().mock_auths(&[MockAuth {
                address: caller.as_ref(),
                invoke: &MockAuthInvoke {
                    contract: &self.contract_id,
                    fn_name,
                    args: args.into_val(self.env.inner()),
                    sub_invokes: &[],
                },
            }]);
        }

        /// Require explicit mocked auth entries for every `require_auth` call.
        ///
        /// Crucible test accounts are `MockAuthContract` addresses whose
        /// `__check_auth` succeeds by default, so this enables strict auth
        /// checking when testing missing-authorization paths.
        fn enforce_auth(&self) {
            self.env.inner().mock_auths(&[]);
        }
    }

    #[test]
    fn test_admin_can_set_increase_and_decrease_reputation() {
        let f = Fixture::setup();

        f.client.with_mock_all_auths(|c| {
            c.set_reputation(&f.admin.address(), &f.user.address(), 100);
            assert_eq!(c.get_reputation(&f.user.address()), 100);

            c.increase_reputation(&f.admin.address(), &f.user.address(), 50);
            assert_eq!(c.get_reputation(&f.user.address()), 150);

            c.decrease_reputation(&f.admin.address(), &f.user.address(), 30);
            assert_eq!(c.get_reputation(&f.user.address()), 120);
        });
    }

    #[test]
    fn test_non_admin_cannot_set_reputation() {
        let f = Fixture::setup();
        f.mock_auth_for(
            &f.user,
            "set_reputation",
            (f.user.address(), f.user.address(), 10_i32),
        );

        assert_reverts!(
            f.client
                .set_reputation(&f.user.address(), &f.user.address(), 10),
            "not admin"
        );
        assert_eq!(f.client.get_reputation(&f.user.address()), 0);
    }

    #[test]
    fn test_non_admin_cannot_increase_reputation() {
        let f = Fixture::setup();
        f.client.with_mock_all_auths(|c| {
            c.set_reputation(&f.admin.address(), &f.user.address(), 100);
        });

        f.mock_auth_for(
            &f.user,
            "increase_reputation",
            (f.user.address(), f.user.address(), 10_i32),
        );

        assert_reverts!(
            f.client
                .increase_reputation(&f.user.address(), &f.user.address(), 10),
            "not admin"
        );
        assert_eq!(f.client.get_reputation(&f.user.address()), 100);
    }

    #[test]
    fn test_non_admin_cannot_decrease_reputation() {
        let f = Fixture::setup();
        f.client.with_mock_all_auths(|c| {
            c.set_reputation(&f.admin.address(), &f.user.address(), 100);
        });

        f.mock_auth_for(
            &f.user,
            "decrease_reputation",
            (f.user.address(), f.user.address(), 10_i32),
        );

        assert_reverts!(
            f.client
                .decrease_reputation(&f.user.address(), &f.user.address(), 10),
            "not admin"
        );
        assert_eq!(f.client.get_reputation(&f.user.address()), 100);
    }

    #[test]
    fn test_mutations_require_auth_without_mock_all_auths() {
        let f = Fixture::setup();
        f.enforce_auth();

        assert_reverts!(
            f.client
                .set_reputation(&f.admin.address(), &f.user.address(), 100),
            "missing auth"
        );
        assert_reverts!(
            f.client
                .increase_reputation(&f.admin.address(), &f.user.address(), 10),
            "missing auth"
        );
        assert_reverts!(
            f.client
                .decrease_reputation(&f.admin.address(), &f.user.address(), 10),
            "missing auth"
        );
    }
}
