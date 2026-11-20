use std::rc::Rc;

use speedyspec_firewall::{
    BanStore, CounterStore, FirewallLog, FirewallSettings, RequestFacts, Verdict,
};

use crate::rules::{
    AllowListRule, BanRule, BlockListRule, LoginLockoutRule, PatternRules, RateLimitRule,
    RequestRule, UserEnumerationRule, XmlRpcRule,
};

/// Which kind of app a firewall guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirewallScope {
    /// The blog and the front-end API: every rule.
    Public,
    /// The admin API: no rate limit and no IP bans, so the block editor's bursts of requests can never lock the
    /// site's owner out of their own admin. Lists, patterns and the login lockout still apply.
    Admin,
}

/// The rules in order (Chain of Responsibility). `Allow` ends the chain; the first `Block` wins; a rule that fails
/// is logged and counts as `Continue`, so the firewall fails open.
pub struct Firewall {
    rules: Vec<Rc<dyn RequestRule>>,
    log: Rc<dyn FirewallLog>,
}

impl Firewall {
    pub fn new(rules: Vec<Rc<dyn RequestRule>>, log: Rc<dyn FirewallLog>) -> Firewall {
        Firewall { rules, log }
    }

    /// Only the rules the settings turn on, in the firewall's fixed order. Disabled settings give no rules.
    pub fn from_settings(
        settings: &FirewallSettings,
        scope: FirewallScope,
        counters: Rc<dyn CounterStore>,
        bans: Rc<dyn BanStore>,
        log: Rc<dyn FirewallLog>,
    ) -> Firewall {
        let mut rules: Vec<Rc<dyn RequestRule>> = Vec::new();
        if settings.enabled {
            if !settings.allow.is_empty() {
                rules.push(Rc::new(AllowListRule(settings.allow.clone())));
            }
            if scope == FirewallScope::Public {
                rules.push(Rc::new(BanRule(bans.clone())));
            }
            if !settings.block.is_empty() {
                rules.push(Rc::new(BlockListRule(settings.block.clone())));
            }
            if !settings.patterns.is_empty() {
                rules.push(Rc::new(PatternRules(settings.patterns.clone())));
            }
            if settings.block_xmlrpc {
                rules.push(Rc::new(XmlRpcRule));
            }
            if settings.block_user_enumeration {
                rules.push(Rc::new(UserEnumerationRule));
            }
            if let Some(policy) = &settings.login {
                rules.push(Rc::new(LoginLockoutRule {
                    policy: policy.clone(),
                    bans: bans.clone(),
                }));
            }
            if let (Some(limit), FirewallScope::Public) = (settings.rate, scope) {
                rules.push(Rc::new(RateLimitRule {
                    limit,
                    counters,
                    bans,
                }));
            }
        }
        Firewall::new(rules, log)
    }

    pub fn check(&self, facts: &RequestFacts) -> Verdict {
        for rule in &self.rules {
            match rule.check(facts) {
                Ok(Verdict::Continue) => {}
                Ok(verdict) => return verdict,
                Err(error) => self.log.warn(&format!("{}: {error}", rule.id())),
            }
        }
        Verdict::Continue
    }
}
