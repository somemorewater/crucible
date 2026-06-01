#[cfg(test)]
mod tests {
    use super::*;
    use crucible::prelude::*;
    use soroban_sdk::{Address, Symbol, Vec};

    fn setup_env() -> MockEnv {
        MockEnv::builder()
            .with_contract::<Treasury>()
            .with_token::<MockToken>() // ensure token availability
            .build()
    }

    #[test]
    fn test_initialize_and_admins() {
        let env = setup_env();
        let admin1 = env.account("admin1");
        let admin2 = env.account("admin2");
        let admins = Vec::from_array(&env, &[admin1.address(), admin2.address()]);
        Treasury::initialize(env.inner(), admins.clone(), 2);
        // verify stored admins and quorum via balance query (no direct getter, rely on no panic)
        // deposit by any user should work
        Treasury::deposit(env.inner(), Address::from([0;32]), 1_000);
        let bal = Treasury::balance_of(env.inner(), admin1.address(), Address::from([0;32]));
        assert_eq!(bal, 1_000);
    }

    #[test]
    #[should_panic]
    fn test_withdraw_insufficient_quorum() {
        let env = setup_env();
        let admin1 = env.account("admin1");
        let admin2 = env.account("admin2");
        let admins = Vec::from_array(&env, &[admin1.address(), admin2.address()]);
        Treasury::initialize(env.inner(), admins.clone(), 2);
        Treasury::deposit(env.inner(), Address::from([0;32]), 1_000);
        // attempt withdraw with only one signer
        Treasury::withdraw(
            env.inner(),
            admin1.address(),
            Address::from([0;32]),
            500,
            Vec::from_array(&env, &[admin1.address()]),
        );
    }

    #[test]
    fn test_successful_withdraw() {
        let env = setup_env();
        let admin1 = env.account("admin1");
        let admin2 = env.account("admin2");
        let admins = Vec::from_array(&env, &[admin1.address(), admin2.address()]);
        Treasury::initialize(env.inner(), admins.clone(), 2);
        Treasury::deposit(env.inner(), Address::from([0;32]), 1_000);
        // withdraw with both admins
        Treasury::withdraw(
            env.inner(),
            admin1.address(),
            Address::from([0;32]),
            400,
            Vec::from_array(&env, &[admin1.address(), admin2.address()]),
        );
        let bal = Treasury::balance_of(env.inner(), admin1.address(), Address::from([0;32]));
        assert_eq!(bal, 600);
    }
}
