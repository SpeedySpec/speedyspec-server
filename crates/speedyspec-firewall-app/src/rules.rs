use std::rc::Rc;

use speedyspec_firewall::{
    BanStore, CounterStore, FirewallError, IpSet, LoginPolicy, PatternRule, RateLimit,
    RequestFacts, Verdict,
};

/// One link in the firewall's chain.
pub trait RequestRule {
    fn id(&self) -> &'static str;
    fn check(&self, facts: &RequestFacts) -> Result<Verdict, FirewallError>;
}

fn forbidden(reason: impl Into<String>) -> Verdict {
    Verdict::Block {
        status: 403,
        retry_after: None,
        reason: reason.into(),
    }
}

pub struct AllowListRule(pub IpSet);

impl RequestRule for AllowListRule {
    fn id(&self) -> &'static str {
        "allow-list"
    }
    fn check(&self, facts: &RequestFacts) -> Result<Verdict, FirewallError> {
        Ok(if self.0.contains(facts.client) {
            Verdict::Allow
        } else {
            Verdict::Continue
        })
    }
}

pub struct BanRule(pub Rc<dyn BanStore>);

impl RequestRule for BanRule {
    fn id(&self) -> &'static str {
        "ban"
    }
    fn check(&self, facts: &RequestFacts) -> Result<Verdict, FirewallError> {
        Ok(match self.0.banned(&format!("ip:{}", facts.client))? {
            Some(reason) => forbidden(format!("banned: {reason}")),
            None => Verdict::Continue,
        })
    }
}

pub struct BlockListRule(pub IpSet);

impl RequestRule for BlockListRule {
    fn id(&self) -> &'static str {
        "block-list"
    }
    fn check(&self, facts: &RequestFacts) -> Result<Verdict, FirewallError> {
        Ok(if self.0.contains(facts.client) {
            forbidden("block list")
        } else {
            Verdict::Continue
        })
    }
}

pub struct PatternRules(pub Vec<PatternRule>);

impl RequestRule for PatternRules {
    fn id(&self) -> &'static str {
        "patterns"
    }
    fn check(&self, facts: &RequestFacts) -> Result<Verdict, FirewallError> {
        Ok(match self.0.iter().find(|rule| rule.matches(facts)) {
            Some(rule) => Verdict::Block {
                status: rule.status,
                retry_after: None,
                reason: format!("pattern {}", rule.regex.as_str()),
            },
            None => Verdict::Continue,
        })
    }
}

pub struct XmlRpcRule;

impl RequestRule for XmlRpcRule {
    fn id(&self) -> &'static str {
        "xmlrpc"
    }
    fn check(&self, facts: &RequestFacts) -> Result<Verdict, FirewallError> {
        Ok(if facts.path == "/xmlrpc.php" {
            forbidden("xml-rpc")
        } else {
            Verdict::Continue
        })
    }
}

pub struct UserEnumerationRule;

impl RequestRule for UserEnumerationRule {
    fn id(&self) -> &'static str {
        "user-enumeration"
    }
    fn check(&self, facts: &RequestFacts) -> Result<Verdict, FirewallError> {
        let admin_screen = facts.path.starts_with("/wp-admin/");
        let author_query = !admin_screen
            && query_values(&facts.query, "author")
                .any(|value| value.trim_start().starts_with(|c: char| c.is_ascii_digit()));
        let users_route = !facts.has_rest_credentials
            && (is_users_route(&facts.path)
                || query_values(&facts.query, "rest_route").any(|route| {
                    is_users_route(&format!("/wp-json{}", route.trim_end_matches('/')))
                }));
        Ok(if author_query || users_route {
            forbidden("user enumeration")
        } else {
            Verdict::Continue
        })
    }
}

fn is_users_route(path: &str) -> bool {
    path == "/wp-json/wp/v2/users" || path.starts_with("/wp-json/wp/v2/users/")
}

/// The percent-decoded values of every `name=` pair in a query string.
fn query_values<'a>(query: &'a str, name: &'a str) -> impl Iterator<Item = String> + 'a {
    query.split('&').filter_map(move |pair| {
        let (key, value) = pair.split_once('=')?;
        (percent_decode(key) == name).then(|| percent_decode(value))
    })
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(high), Some(low)) => {
                    out.push((high * 16 + low) as u8);
                    i += 3;
                    continue;
                }
                _ => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct LoginLockoutRule {
    pub policy: LoginPolicy,
    pub bans: Rc<dyn BanStore>,
}

impl RequestRule for LoginLockoutRule {
    fn id(&self) -> &'static str {
        "login-lockout"
    }
    fn check(&self, facts: &RequestFacts) -> Result<Verdict, FirewallError> {
        if facts.method != "POST" || !self.policy.signin_paths.contains(&facts.path) {
            return Ok(Verdict::Continue);
        }
        Ok(
            match self.bans.banned(&format!("login:{}", facts.client))? {
                Some(_) => Verdict::Block {
                    status: 429,
                    retry_after: Some(self.policy.lockout),
                    reason: "login lockout".into(),
                },
                None => Verdict::Continue,
            },
        )
    }
}

pub struct RateLimitRule {
    pub limit: RateLimit,
    pub counters: Rc<dyn CounterStore>,
    pub bans: Rc<dyn BanStore>,
}

impl RequestRule for RateLimitRule {
    fn id(&self) -> &'static str {
        "rate-limit"
    }
    fn check(&self, facts: &RequestFacts) -> Result<Verdict, FirewallError> {
        let count = self
            .counters
            .hit(&format!("rate:{}", facts.client), self.limit.window)?;
        if count <= self.limit.requests {
            return Ok(Verdict::Continue);
        }
        if let Some(ban_for) = self.limit.ban_for {
            self.bans
                .ban(&format!("ip:{}", facts.client), ban_for, "rate limit")?;
        }
        Ok(Verdict::Block {
            status: 429,
            retry_after: Some(self.limit.ban_for.unwrap_or(self.limit.window)),
            reason: "rate limit".into(),
        })
    }
}
