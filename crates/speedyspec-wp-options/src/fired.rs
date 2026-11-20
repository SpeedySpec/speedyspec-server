//! The hooks a `get_option()` would fire now, listed without firing any (for callers deciding whether a read can
//! stay in Rust).

use speedyspec_wp_hooks::{HookName, HookValue};

use crate::OptionName;
use crate::api::{OptionsApi, isset};

impl OptionsApi {
    /// The hooks `get_option(name)` could fire now, in `option.php`'s order of mention, without firing any or
    /// counting a cache hit or miss. `query` is listed whenever the read could reach the database.
    pub fn hooks_fired_by_get(&self, name: &OptionName) -> Vec<HookName> {
        let mut hooks: Vec<String> = Vec::new();
        self.list_get_hooks(name, &mut hooks);
        hooks.iter().map(|h| HookName::from(h.as_str())).collect()
    }

    fn list_get_hooks(&self, name: &OptionName, hooks: &mut Vec<String>) {
        let installing = self.d.installing.installing();
        if let Some(new) = name.deprecated_replacement()
            && !installing
        {
            if let Some(new) = OptionName::new(new) {
                self.list_get_hooks(&new, hooks);
            }
            return;
        }
        let n = name.as_str();
        let mut push = |hook: String| {
            if !hooks.contains(&hook) {
                hooks.push(hook);
            }
        };
        push(format!("pre_option_{n}"));
        push("pre_option".into());
        push(format!("default_option_{n}"));
        push(format!("option_{n}"));
        let group = Self::group();
        if installing {
            push("query".into());
        } else {
            push("pre_wp_load_alloptions".into());
            push("alloptions".into());
            let alloptions = self
                .cache()
                .peek(&HookValue::Str("alloptions".into()), &group);
            let cached = alloptions
                .as_ref()
                .is_some_and(|a| crate::api::truthy(&a.read()));
            if !cached {
                push("wp_autoload_values_to_autoload".into());
                push("pre_cache_alloptions".into());
                push("query".into());
            }
            let in_alloptions = alloptions.is_some_and(|a| isset(&a.read(), n).is_some());
            let known_missing = self
                .cache()
                .peek(&HookValue::Str("notoptions".into()), &group)
                .is_some_and(|m| isset(&m.read(), n).is_some());
            let single = self.cache().peek(&HookValue::Str(n.to_string()), &group);
            let single = single.is_some_and(|v| !matches!(v, HookValue::Bool(false)));
            if !in_alloptions && !known_missing && !single {
                push("query".into());
            }
        }
        if n == "home"
            && let Some(siteurl) = OptionName::new("siteurl")
        {
            self.list_get_hooks(&siteurl, hooks);
        }
    }
}
