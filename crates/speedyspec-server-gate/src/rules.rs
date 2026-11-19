//! A release's access rules: which paths behind a gated mount are public, under which condition, and which are one
//! page's own files.

use regex::Regex;
use speedyspec_server_domain::{AccessRule, PublicWhen};

use crate::calls::SiteState;

/// What a request's path is, by its release's rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleOutcome {
    /// Anyone may load it.
    Public,
    /// A conditional public entry whose condition does not hold: answered 404 for everyone.
    Disabled,
    /// One page's files: only a signed-in user whose session lists the page.
    Page(String),
    /// Any signed-in user.
    SignedIn,
}

/// How a page id is spelled: two lowercase segments, `<namespace>/<name>`.
const PAGE_ID: &str = "[a-z0-9][a-z0-9-]*/[a-z0-9][a-z0-9-]*";

struct Compiled {
    regex: Regex,
    public: Option<PublicWhen>,
    captures_page: bool,
}

/// A release's rules, compiled.
pub struct AccessRules {
    rules: Vec<Compiled>,
}

impl AccessRules {
    /// Compiles every rule. A pattern is a glob over the path relative to the mount, without its leading `/`: `**`
    /// matches anything (across `/`), `*` anything within one segment, `{page}` a page id (once at most, and only
    /// with `"page": "{page}"`), and `""` only the mount's root.
    pub fn compile(rules: &[AccessRule]) -> Result<AccessRules, String> {
        let rules = rules
            .iter()
            .map(|rule| {
                let captures_page = rule.pattern.contains("{page}");
                if rule.pattern.matches("{page}").count() > 1 {
                    return Err(format!("access rule {:?} names {{page}} more than once", rule.pattern));
                }
                match (&rule.page, captures_page) {
                    (Some(page), true) if page == "{page}" => {}
                    (Some(page), _) => {
                        return Err(format!(
                            "access rule {:?}: `page` must be \"{{page}}\", captured by the pattern (got {page:?})",
                            rule.pattern
                        ));
                    }
                    (None, true) => {
                        return Err(format!("access rule {:?} captures {{page}} but is not a page rule", rule.pattern));
                    }
                    (None, false) => {}
                }
                Ok(Compiled {
                    regex: Regex::new(&glob_regex(&rule.pattern))
                        .map_err(|err| format!("access rule {:?}: {err}", rule.pattern))?,
                    public: rule.public,
                    captures_page,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(AccessRules { rules })
    }

    /// What `rel_path` is, given the site's state and whether its app is in maintenance. The first matching rule wins.
    pub fn decide(&self, rel_path: &str, site: SiteState, maintenance: bool) -> RuleOutcome {
        for rule in &self.rules {
            let Some(found) = rule.regex.captures(rel_path) else {
                continue;
            };
            if rule.captures_page {
                return RuleOutcome::Page(found["page"].to_string());
            }
            let holds = match rule.public.expect("a rule is public or a page rule") {
                PublicWhen::Always => true,
                PublicWhen::NotInstalled => !site.installed,
                PublicWhen::Maintenance => maintenance,
            };
            return if holds {
                RuleOutcome::Public
            } else {
                RuleOutcome::Disabled
            };
        }
        RuleOutcome::SignedIn
    }
}

/// A glob as an anchored regular expression.
fn glob_regex(pattern: &str) -> String {
    let mut out = String::from("^");
    let mut rest = pattern;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("{page}") {
            out.push_str(&format!("(?P<page>{PAGE_ID})"));
            rest = after;
        } else if let Some(after) = rest.strip_prefix("**") {
            out.push_str(".*");
            rest = after;
        } else if let Some(after) = rest.strip_prefix('*') {
            out.push_str("[^/]*");
            rest = after;
        } else {
            let c = rest.chars().next().expect("not empty");
            out.push_str(&regex::escape(&c.to_string()));
            rest = &rest[c.len_utf8()..];
        }
    }
    out.push('$');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use speedyspec_server_domain::AccessRule;

    fn rules() -> AccessRules {
        AccessRules::compile(
            &serde_json::from_str::<Vec<AccessRule>>(
                r#"[
  {"match":"","public":"always"},{"match":"signin","public":"always"},{"match":"signin/**","public":"always"},
  {"match":"install/**","public":"not-installed"},
  {"match":"components/{page}/**","page":"{page}"},{"match":"contracts/{page}/**","page":"{page}"}]"#,
            )
            .unwrap(),
        )
        .unwrap()
    }

    const INSTALLED: SiteState = SiteState {
        installed: true,
        maintenance: false,
    };

    #[test]
    fn root_and_signin_are_public() {
        assert_eq!(rules().decide("", INSTALLED, false), RuleOutcome::Public);
        assert_eq!(
            rules().decide("signin", INSTALLED, false),
            RuleOutcome::Public
        );
        assert_eq!(
            rules().decide("signin/app.js", INSTALLED, false),
            RuleOutcome::Public
        );
    }

    #[test]
    fn install_is_disabled_once_installed() {
        assert_eq!(
            rules().decide("install/index.html", INSTALLED, false),
            RuleOutcome::Disabled
        );
        assert_eq!(
            rules().decide(
                "install/index.html",
                SiteState {
                    installed: false,
                    maintenance: false
                },
                false
            ),
            RuleOutcome::Public
        );
    }

    #[test]
    fn maintenance_entries_follow_maintenance() {
        let rules = AccessRules::compile(
            &serde_json::from_str::<Vec<AccessRule>>(
                r#"[{"match":"down/**","public":"maintenance"}]"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            rules.decide("down/page.html", INSTALLED, true),
            RuleOutcome::Public
        );
        assert_eq!(
            rules.decide("down/page.html", INSTALLED, false),
            RuleOutcome::Disabled
        );
    }

    #[test]
    fn page_files_capture_the_page() {
        assert_eq!(
            rules().decide("components/core/general/view.js", INSTALLED, false),
            RuleOutcome::Page("core/general".into())
        );
        assert_eq!(
            rules().decide("contracts/my-plugin/settings2/index.js", INSTALLED, false),
            RuleOutcome::Page("my-plugin/settings2".into())
        );
    }

    #[test]
    fn three_segment_page_never_matches() {
        assert_eq!(
            rules().decide("components/core/a/b/view.js", INSTALLED, false),
            RuleOutcome::Page("core/a".into())
        );
    }

    #[test]
    fn a_page_id_must_be_well_formed() {
        assert_eq!(
            rules().decide("components/Core/a/view.js", INSTALLED, false),
            RuleOutcome::SignedIn
        );
        assert_eq!(
            rules().decide("components/-x/a/view.js", INSTALLED, false),
            RuleOutcome::SignedIn
        );
    }

    #[test]
    fn anything_else_needs_sign_in() {
        assert_eq!(
            rules().decide("kit/frame.js", INSTALLED, false),
            RuleOutcome::SignedIn
        );
        assert_eq!(
            rules().decide("signinx", INSTALLED, false),
            RuleOutcome::SignedIn
        );
        assert_eq!(
            rules().decide("x/signin", INSTALLED, false),
            RuleOutcome::SignedIn
        );
    }

    #[test]
    fn first_match_wins() {
        let rules = AccessRules::compile(
            &serde_json::from_str::<Vec<AccessRule>>(
                r#"[{"match":"components/core/help/**","public":"always"},{"match":"components/{page}/**","page":"{page}"}]"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            rules.decide("components/core/help/view.js", INSTALLED, false),
            RuleOutcome::Public
        );
        assert_eq!(
            rules.decide("components/core/users/view.js", INSTALLED, false),
            RuleOutcome::Page("core/users".into())
        );
    }

    #[test]
    fn bad_rules_are_refused() {
        for json in [
            r#"[{"match":"a/{page}/{page}","page":"{page}"}]"#,
            r#"[{"match":"a/{page}","page":"x"}]"#,
            r#"[{"match":"a/**","page":"{page}"}]"#,
        ] {
            let rules: Vec<AccessRule> = serde_json::from_str(json).unwrap();
            assert!(AccessRules::compile(&rules).is_err(), "{json}");
        }
    }
}
