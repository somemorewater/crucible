#![cfg(test)]
extern crate std;

use crucible::prelude::*;
use crucible::{assert_emitted, assert_not_emitted, assert_reverts};
use soroban_sdk::{symbol_short, Address};

use crate::{Counter, CounterClient};

// ---------------------------------------------------------------------------
// Test fixture
// ---------------------------------------------------------------------------

#[fixture]
struct Ctx {
    pub env: MockEnv,
    pub id: Address,
}

impl Ctx {
    pub fn setup() -> Self {
        let env = MockEnv::builder().with_contract::<Counter>().build();
        let id = env.contract_id::<Counter>();
        Ctx { env, id }
    }

    fn client(&self) -> CounterClient<'_> {
        CounterClient::new(self.env.inner(), &self.id)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_initial_value_is_zero() {
    let f = Ctx::setup();
    assert_eq!(f.client().get(), 0);
}

#[test]
fn test_increment_increases_count() {
    let f = Ctx::setup();
    let c = f.client();
    assert_eq!(c.increment(), 1);
    assert_eq!(c.increment(), 2);
    assert_eq!(c.increment(), 3);
    assert_eq!(c.get(), 3);
}

#[test]
fn test_increment_by_bulk() {
    let f = Ctx::setup();
    let c = f.client();
    assert_eq!(c.increment_by(&5), 5);
    assert_eq!(c.increment_by(&10), 15);
    assert_eq!(c.get(), 15);
}

#[test]
fn test_decrement_decreases_count() {
    let f = Ctx::setup();
    let c = f.client();
    c.increment();
    c.increment();
    assert_eq!(c.decrement(), 1);
    assert_eq!(c.decrement(), 0);
}

#[test]
fn test_decrement_at_zero_reverts() {
    let f = Ctx::setup();
    assert_reverts!(f.client().decrement(), "underflow");
}

#[test]
fn test_reset_clears_count() {
    let f = Ctx::setup();
    let c = f.client();
    c.increment_by(&42);
    assert_eq!(c.get(), 42);
    c.reset();
    assert_eq!(c.get(), 0);
}

#[test]
fn test_initialize_sets_starting_value() {
    let f = Ctx::setup();
    let c = f.client();
    c.initialize(&10);
    assert_eq!(c.get(), 10);
    assert_eq!(c.increment(), 11);
}

#[test]
fn test_double_initialize_reverts() {
    let f = Ctx::setup();
    let c = f.client();
    c.initialize(&5);
    assert_reverts!(c.initialize(&10), "already initialized");
}

#[test]
fn test_increment_emits_event() {
    let f = Ctx::setup();
    f.client().increment();
    assert_emitted!(f.env, f.id, (symbol_short!("incr"),), 1_u32);
}

#[test]
fn test_reset_emits_event() {
    let f = Ctx::setup();
    f.client().reset();
    assert_emitted!(f.env, f.id, (symbol_short!("reset"),), ());
}

#[test]
fn test_get_emits_no_event() {
    let f = Ctx::setup();
    f.client().get();
    assert_not_emitted!(f.env);
}

#[test]
fn test_fixture_reset_restores_state() {
    let mut f = Ctx::setup();
    f.client().increment_by(&99);
    assert_eq!(f.client().get(), 99);

    // Reset fixture to a clean state
    f.reset();
    assert_eq!(f.client().get(), 0);
}
