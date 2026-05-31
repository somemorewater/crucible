//! Test utilities for creating domain objects in tests.
//!
//! This module provides factory functions for creating common domain objects
//! used throughout the test suite. Factories help reduce boilerplate and ensure
//! consistent test data creation.
//!
//! # Usage
//!
//! ```rust
//! use backend::test_utils::factories;
//!
//! // Create a user with default values
//! let user = factories::user::create();
//!
//! // Create a user with custom values
//! let user = factories::user::create_with(|u| {
//!     u.email = "test@example.com".to_string();
//!     u.is_active = false;
//! });
//!
//! // Build a user step by step
//! let user = factories::user::build()
//!     .email("custom@example.com")
//!     .is_admin(true)
//!     .finish();
//! ```

pub mod order;
pub mod product;
pub mod session;
pub mod user;

pub use order::{
    build as build_order, create as create_order, create_for_user as create_order_for_user,
    create_many as create_orders, create_many_with as create_orders_with,
    create_with as create_order_with, create_with_items as create_order_with_items, OrderFactory,
    OrderItem, OrderStatus, PaymentMethod,
};
pub use product::{
    build as build_product, create as create_product, create_many as create_products,
    create_many_with as create_products_with, create_with as create_product_with, Availability,
    ProductCategory, ProductFactory,
};
pub use session::{
    build as build_session, create as create_session, create_for_user as create_session_for_user,
    create_many as create_sessions, create_many_for_user as create_sessions_for_user,
    create_many_with as create_sessions_with, create_with as create_session_with, SessionFactory,
    SessionStatus, SessionType,
};
pub use user::{
    build as build_user, create as create_user, create_many as create_users,
    create_many_with as create_users_with, create_with as create_user_with, Role, UserFactory,
};

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Complete domain fixture used by backend tests.
#[derive(Debug, Clone)]
pub struct TestFixture {
    /// Users in the fixture.
    pub users: Vec<user::User>,
    /// Products in the fixture.
    pub products: Vec<product::Product>,
    /// Orders linked to fixture users.
    pub orders: Vec<order::Order>,
    /// Sessions linked to fixture users.
    pub sessions: Vec<session::Session>,
    /// Stable creation timestamp applied to all generated entities.
    pub created_at: DateTime<Utc>,
}

impl TestFixture {
    /// Returns the first user id, if a user exists.
    pub fn primary_user_id(&self) -> Option<Uuid> {
        self.users.first().map(|user| user.id)
    }
}

/// Builder for cohesive backend test fixtures.
///
/// The factory composes the existing domain-specific factories while keeping
/// relationships valid: orders and sessions always reference generated users.
/// Complexity is O(u + p + u * (o + s)) time and space, where `u` is user
/// count, `p` is product count, `o` is orders per user, and `s` is sessions per
/// user.
#[derive(Debug, Clone)]
pub struct FixtureFactory {
    user_count: usize,
    product_count: usize,
    orders_per_user: usize,
    sessions_per_user: usize,
    created_at: DateTime<Utc>,
}

impl Default for FixtureFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl FixtureFactory {
    /// Creates a factory with a minimal but useful default fixture shape.
    pub fn new() -> Self {
        Self {
            user_count: 1,
            product_count: 1,
            orders_per_user: 1,
            sessions_per_user: 1,
            created_at: Utc::now(),
        }
    }

    /// Sets how many users to create.
    pub fn users(mut self, count: usize) -> Self {
        self.user_count = count;
        self
    }

    /// Sets how many products to create.
    pub fn products(mut self, count: usize) -> Self {
        self.product_count = count;
        self
    }

    /// Sets how many orders to create for each generated user.
    pub fn orders_per_user(mut self, count: usize) -> Self {
        self.orders_per_user = count;
        self
    }

    /// Sets how many sessions to create for each generated user.
    pub fn sessions_per_user(mut self, count: usize) -> Self {
        self.sessions_per_user = count;
        self
    }

    /// Sets a stable timestamp for generated entities.
    pub fn created_at(mut self, created_at: DateTime<Utc>) -> Self {
        self.created_at = created_at;
        self
    }

    /// Builds the fixture.
    pub fn build(self) -> TestFixture {
        let users = self.build_users();
        let products = self.build_products();
        let orders = self.build_orders(&users, &products);
        let sessions = self.build_sessions(&users);

        TestFixture {
            users,
            products,
            orders,
            sessions,
            created_at: self.created_at,
        }
    }

    fn build_users(&self) -> Vec<user::User> {
        (0..self.user_count)
            .map(|idx| {
                user::build()
                    .email(format!("fixture-user-{idx}@example.com"))
                    .username(format!("fixture_user_{idx}"))
                    .created_at(self.created_at)
                    .finish()
            })
            .collect()
    }

    fn build_products(&self) -> Vec<product::Product> {
        (0..self.product_count)
            .map(|idx| {
                product::build()
                    .name(format!("Fixture Product {idx}"))
                    .sku(format!("FIXTURE-SKU-{idx}"))
                    .created_at(self.created_at)
                    .updated_at(self.created_at)
                    .finish()
            })
            .collect()
    }

    fn build_orders(
        &self,
        users: &[user::User],
        products: &[product::Product],
    ) -> Vec<order::Order> {
        if products.is_empty() {
            return Vec::new();
        }

        users
            .iter()
            .flat_map(|user| {
                (0..self.orders_per_user).map(move |idx| {
                    let product = &products[idx % products.len()];
                    let item = order::OrderItem::new(
                        product.id,
                        product.name.clone(),
                        1,
                        product.price_cents,
                    );

                    order::build()
                        .user_id(user.id)
                        .add_item(item)
                        .created_at(self.created_at)
                        .updated_at(self.created_at)
                        .finish()
                })
            })
            .collect()
    }

    fn build_sessions(&self, users: &[user::User]) -> Vec<session::Session> {
        users
            .iter()
            .flat_map(|user| {
                (0..self.sessions_per_user).map(move |_| {
                    session::build()
                        .user_id(user.id)
                        .created_at(self.created_at)
                        .last_used_at(self.created_at)
                        .finish()
                })
            })
            .collect()
    }
}

/// Creates the default cohesive test fixture.
pub fn create_fixture() -> TestFixture {
    FixtureFactory::new().build()
}

/// Returns a builder for cohesive backend test fixtures.
pub fn build_fixture() -> FixtureFactory {
    FixtureFactory::new()
}

#[cfg(test)]
mod fixture_factory_tests {
    use super::*;

    #[test]
    fn default_fixture_has_related_domain_objects() {
        let fixture = create_fixture();

        assert_eq!(fixture.users.len(), 1);
        assert_eq!(fixture.products.len(), 1);
        assert_eq!(fixture.orders.len(), 1);
        assert_eq!(fixture.sessions.len(), 1);
        assert_eq!(fixture.orders[0].user_id, fixture.users[0].id);
        assert_eq!(fixture.sessions[0].user_id, fixture.users[0].id);
    }

    #[test]
    fn fixture_counts_scale_by_user_relationships() {
        let fixture = build_fixture()
            .users(3)
            .products(2)
            .orders_per_user(2)
            .sessions_per_user(4)
            .build();

        assert_eq!(fixture.users.len(), 3);
        assert_eq!(fixture.products.len(), 2);
        assert_eq!(fixture.orders.len(), 6);
        assert_eq!(fixture.sessions.len(), 12);
    }

    #[test]
    fn no_products_means_no_orders() {
        let fixture = build_fixture()
            .users(2)
            .products(0)
            .orders_per_user(3)
            .build();

        assert_eq!(fixture.orders.len(), 0);
        assert_eq!(fixture.sessions.len(), 2);
    }

    #[test]
    fn fixed_timestamp_is_applied() {
        let now = Utc::now();
        let fixture = build_fixture().created_at(now).build();

        assert_eq!(fixture.created_at, now);
        assert_eq!(fixture.users[0].created_at, now);
        assert_eq!(fixture.products[0].created_at, now);
        assert_eq!(fixture.orders[0].created_at, now);
        assert_eq!(fixture.sessions[0].created_at, now);
    }
}
