//! The firewall's use cases: the rules as a Chain of Responsibility ([`Firewall`]), the settings cache that
//! rebuilds the chain only when the settings change ([`FirewallCache`]), and the login guard that counts failed
//! sign-ins and locks a client out ([`LoginGuard`]).

mod cache;
mod firewall;
mod login;
mod rules;

pub use cache::FirewallCache;
pub use firewall::Firewall;
pub use login::LoginGuard;
pub use rules::{
    AllowListRule, BanRule, BlockListRule, LoginLockoutRule, PatternRules, RateLimitRule,
    RequestRule, UserEnumerationRule, XmlRpcRule,
};
