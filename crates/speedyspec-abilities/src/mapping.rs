//! The default ability metadata (Strategy): an operation's category, visibility and annotations from its traits.

use speedyspec_abilities_contracts::{
    AbilityBinding, AbilityMapping, Annotations, is_ability_name,
};
use speedyspec_operation_contracts::{Action, Kind, OperationSpec, Value};

/// Publishes operations as abilities with metadata from their traits:
///
/// - **category:** the operation's vendor, as a slug ([`category_slug`]);
/// - **visibility:** `show_in_rest` (every operation is reachable on every transport by default, and its permission
///   check still applies), not `public`;
/// - **annotations:** a query is readonly, destructive no, idempotent yes; a command is not readonly, destructive when
///   the operation is (or deletes), idempotent when the operation is (or updates or deletes);
/// - **label and description:** the operation's labels, else its name.
///
/// Subscriptions, and operations whose name is not an ability name, have no ability.
pub struct DefaultAbilityMapping;

/// A vendor as an ability category slug: lowercase, every run of characters other than letters and digits one dash,
/// no dash at either end.
pub fn category_slug(vendor: &str) -> String {
    let mut slug = String::new();
    for c in vendor.to_ascii_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_owned()
}

fn or_name(text: &str, spec: &OperationSpec) -> String {
    if text.is_empty() {
        spec.name.as_str().to_owned()
    } else {
        text.to_owned()
    }
}

impl AbilityMapping for DefaultAbilityMapping {
    fn binding(&self, spec: &OperationSpec) -> Option<AbilityBinding> {
        if spec.kind == Kind::Subscription || !is_ability_name(spec.name.as_str()) {
            return None;
        }
        let (vendor, _) = spec.name.as_str().split_once('/')?;
        let action = spec.resource.as_ref().map(|r| &r.action);
        let annotations = if spec.kind == Kind::Query {
            Annotations {
                readonly: Some(true),
                destructive: Some(false),
                idempotent: Some(true),
            }
        } else {
            let deletes = matches!(action, Some(Action::Delete));
            let updates = matches!(action, Some(Action::Update));
            Annotations {
                readonly: Some(false),
                destructive: Some(spec.destructive || deletes),
                idempotent: Some(spec.idempotent || updates || deletes),
            }
        };
        Some(AbilityBinding {
            label: Some(or_name(&spec.labels.title, spec)),
            description: Some(or_name(&spec.labels.description, spec)),
            category: Some(category_slug(vendor)),
            show_in_rest: Some(true),
            public: Some(false),
            annotations,
            meta: Value::Null,
        })
    }
}
