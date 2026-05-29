//! Assertion macros for Soroban contract testing.
//!
//! These macros provide ergonomic assertions for common test patterns:
//! - `assert_reverts!` — assert a contract call panics (reverts)
//! - `assert_emitted!` — assert a specific event was emitted
//! - `assert_not_emitted!` — assert no events were emitted

/// Asserts that a contract invocation panics (reverts).
///
/// In Soroban's test environment, contract errors manifest as panics.
/// This macro wraps the expression in [`std::panic::catch_unwind`] and
/// asserts the panic occurred.
///
/// # Example
///
/// ```ignore
/// assert_reverts!(client.transfer(&alice, &bob, &(-1_i128)));
/// assert_reverts!(client.claim(), "too early");
/// ```
#[macro_export]
macro_rules! assert_reverts {
    ($expr:expr) => {{
        extern crate std;
        let __result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            $expr;
        }));
        assert!(
            __result.is_err(),
            "Expected contract call to revert (panic), but it succeeded"
        );
    }};
    ($expr:expr, $msg:literal) => {{
        extern crate std;
        let __result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            $expr;
        }));
        assert!(
            __result.is_err(),
            concat!(
                "Expected contract call to revert, but it succeeded. Context: ",
                $msg
            )
        );
    }};
}

/// Asserts that a specific event was emitted (among any others).
///
/// Checks that the event log contains an entry with the given contract address,
/// topics tuple, and data value. Other events may also be present. Topics are
/// passed as a tuple and converted to `Vec<Val>` via [`soroban_sdk::IntoVal`].
///
/// # Example
///
/// ```ignore
/// client.increment();
/// assert_emitted!(
///     env,
///     contract_id,
///     (symbol_short!("incr"),),
///     1_u32
/// );
/// ```
#[macro_export]
macro_rules! assert_emitted {
    ($env:expr, $contract_id:expr, $topics:expr, $data:expr) => {{
        use soroban_sdk::testutils::Events as _;
        use soroban_sdk::IntoVal as _;
        let __env = $env.inner();
        let __events = __env.events().all();
        let __expected_vec = soroban_sdk::vec![
            __env,
            (
                $contract_id.clone(),
                ($topics).into_val(__env),
                ($data).into_val(__env),
            )
        ];
        assert_eq!(
            __events, __expected_vec,
            "Expected event log to match exactly. Events: {:?}",
            __events
        );
    }};
}

/// Asserts that no events were emitted.
///
/// # Example
///
/// ```ignore
/// client.get(); // read-only, no events
/// assert_not_emitted!(env);
/// ```
#[macro_export]
macro_rules! assert_not_emitted {
    ($env:expr) => {{
        use soroban_sdk::testutils::Events as _;
        let __events = $env.inner().events().all();
        assert!(
            __events.events().is_empty(),
            "Expected no events to be emitted, but {} were emitted. Events: {:?}",
            __events.events().len(),
            __events
        );
    }};
}
