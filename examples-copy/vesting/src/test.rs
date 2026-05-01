#![cfg(test)]
extern crate std;

use crucible::assert_reverts;
use crucible::prelude::*;

use crate::{Vesting, VestingClient};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const TOTAL: i128 = 10_000_000; // 10 tokens (7 decimals)
const BASE_TIME: u64 = 1_000_000;
const CLIFF_DAYS: u64 = 30;
const VEST_DAYS: u64 = 180;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct Ctx {
    pub env: MockEnv,
    pub id: soroban_sdk::Address,
    pub admin: AccountHandle,
    pub beneficiary: AccountHandle,
    pub token: MockToken,
}

impl Ctx {
    fn setup() -> Self {
        let env = MockEnv::builder()
            .at_timestamp(BASE_TIME)
            .with_contract::<Vesting>()
            .with_account("admin", Stroops::xlm(100))
            .with_account("beneficiary", Stroops::xlm(10))
            .build();

        let id = env.contract_id::<Vesting>();
        let admin = env.account("admin");
        let beneficiary = env.account("beneficiary");

        let token = MockToken::new(&env, "VEST", 7);
        token.mint(&admin, TOTAL);

        // Initialize the vesting schedule. mock_all_auths() is needed for:
        //   - admin.require_auth() inside initialize()
        //   - token.transfer(admin, contract, total) inside initialize()
        env.mock_all_auths();
        VestingClient::new(env.inner(), &id).initialize(
            &admin,
            &beneficiary,
            &token.address(),
            &TOTAL,
            &BASE_TIME,
            &Duration::days(CLIFF_DAYS).as_seconds(),
            &Duration::days(VEST_DAYS).as_seconds(),
        );

        Ctx {
            env,
            id,
            admin,
            beneficiary,
            token,
        }
    }

    fn client(&self) -> VestingClient<'_> {
        VestingClient::new(self.env.inner(), &self.id)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_nothing_claimable_before_cliff() {
    let ctx = Ctx::setup();
    // Advance to just before the cliff ends.
    ctx.env.advance_time(Duration::days(CLIFF_DAYS - 1));
    assert_eq!(ctx.client().claimable(), 0);
}

#[test]
fn test_nothing_claimable_at_cliff_start() {
    let ctx = Ctx::setup();
    // At exactly the cliff boundary, 0 of the vesting window has elapsed.
    ctx.env.advance_time(Duration::days(CLIFF_DAYS));
    assert_eq!(ctx.client().claimable(), 0);
}

#[test]
fn test_partial_vesting_halfway_through() {
    let ctx = Ctx::setup();
    // Advance to cliff + half the vesting window → 50 % vested.
    ctx.env
        .advance_time(Duration::days(CLIFF_DAYS + VEST_DAYS / 2));
    let claimable = ctx.client().claimable();
    assert_eq!(claimable, TOTAL / 2);
}

#[test]
fn test_full_vesting_after_duration() {
    let ctx = Ctx::setup();
    // Advance past cliff + full vesting window → 100 % vested.
    ctx.env.advance_time(Duration::days(CLIFF_DAYS + VEST_DAYS));
    assert_eq!(ctx.client().claimable(), TOTAL);
}

#[test]
fn test_claim_transfers_tokens_to_beneficiary() {
    let ctx = Ctx::setup();
    ctx.env.advance_time(Duration::days(CLIFF_DAYS + VEST_DAYS));
    ctx.env.mock_all_auths();
    ctx.client().claim();

    assert_eq!(ctx.token.balance(&ctx.beneficiary), TOTAL);
    assert_eq!(ctx.client().claimable(), 0); // nothing left
}

#[test]
fn test_claim_before_cliff_reverts() {
    let ctx = Ctx::setup();
    // Nothing to claim before the cliff.
    ctx.env.mock_all_auths();
    assert_reverts!(ctx.client().claim(), "nothing to claim");
}

#[test]
fn test_partial_claim_then_more() {
    let ctx = Ctx::setup();
    // Claim at 50 %.
    ctx.env
        .advance_time(Duration::days(CLIFF_DAYS + VEST_DAYS / 2));
    ctx.env.mock_all_auths();
    ctx.client().claim();
    assert_eq!(ctx.token.balance(&ctx.beneficiary), TOTAL / 2);

    // Advance to 100 % and claim the rest.
    ctx.env.advance_time(Duration::days(VEST_DAYS / 2));
    ctx.client().claim();
    assert_eq!(ctx.token.balance(&ctx.beneficiary), TOTAL);
}

#[test]
fn test_revoke_returns_unvested_tokens_to_admin() {
    let ctx = Ctx::setup();
    // Revoke at the 50 % mark — admin should receive the unvested half.
    ctx.env
        .advance_time(Duration::days(CLIFF_DAYS + VEST_DAYS / 2));
    let vested_so_far = ctx.client().vested();
    ctx.env.mock_all_auths();
    ctx.client().revoke();

    let unvested = TOTAL - vested_so_far;
    assert_eq!(ctx.token.balance(&ctx.admin), unvested);
}

#[test]
fn test_claim_after_revoke_reverts() {
    let ctx = Ctx::setup();
    ctx.env.advance_time(Duration::days(CLIFF_DAYS + VEST_DAYS));
    ctx.env.mock_all_auths();
    ctx.client().revoke();
    assert_reverts!(ctx.client().claim(), "revoked");
}

#[test]
fn test_vested_increases_monotonically_with_time() {
    let ctx = Ctx::setup();
    let v0 = ctx.client().vested();
    ctx.env.advance_time(Duration::days(CLIFF_DAYS));
    let v1 = ctx.client().vested();
    ctx.env.advance_time(Duration::days(VEST_DAYS / 2));
    let v2 = ctx.client().vested();
    ctx.env.advance_time(Duration::days(VEST_DAYS));
    let v3 = ctx.client().vested();

    assert_eq!(v0, 0);
    assert_eq!(v1, 0); // cliff boundary
    assert!(v2 > v1);
    assert_eq!(v3, TOTAL);
}
