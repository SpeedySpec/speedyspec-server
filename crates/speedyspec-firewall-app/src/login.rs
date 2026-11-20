use std::net::IpAddr;
use std::rc::Rc;

use speedyspec_firewall::{BanStore, CounterStore, FirewallError, LoginPolicy};

/// Counts a client's failed sign-ins and locks it out once they reach the policy's maximum.
pub struct LoginGuard {
    policy: LoginPolicy,
    counters: Rc<dyn CounterStore>,
    bans: Rc<dyn BanStore>,
}

impl LoginGuard {
    pub fn new(policy: LoginPolicy, counters: Rc<dyn CounterStore>, bans: Rc<dyn BanStore>) -> LoginGuard {
        LoginGuard { policy, counters, bans }
    }

    pub fn record_failure(&self, client: IpAddr) -> Result<(), FirewallError> {
        let client = client.to_canonical();
        let failures = self.counters.hit(&format!("login:{client}"), self.policy.window)?;
        if failures >= self.policy.max_failures {
            self.bans.ban(&format!("login:{client}"), self.policy.lockout, "login failures")?;
        }
        Ok(())
    }
}
