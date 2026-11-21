//! `kses_init_filters()`, `kses_remove_filters()` and `kses_init()`.

use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_wp_hooks::{Callback, CallbackId, HookName};
use speedyspec_wp_kses_contracts::KsesError;

use crate::KsesApi;
use crate::api::{bytes_value, hook_error, php_string};

/// A kses filter function registered on a hook, by its WordPress name.
type FilterFn = fn(&KsesApi, &[u8]) -> Result<Vec<u8>, KsesError>;

fn by_name(name: &str) -> FilterFn {
    match name {
        "wp_filter_kses" => KsesApi::filter_kses,
        _ => KsesApi::filter_post_kses,
    }
}

impl KsesApi {
    fn me(&self) -> Result<Rc<KsesApi>, KsesError> {
        self.me.borrow().upgrade().ok_or_else(|| {
            KsesError::Hook("kses filters need a shared KsesApi (KsesApi::shared)".into())
        })
    }

    /// The callback WordPress registers as `$name`: the bridge's own function of that name where there is one (PHP's
    /// `wp_filter_kses`, as WordPress registers it), else the native function, keyed by the same id PHP uses.
    fn native(&self, name: &'static str) -> Result<Callback, KsesError> {
        if let Some(named) = self.d.callbacks.named_filter(name) {
            return Ok(named);
        }
        let me = Rc::downgrade(&self.me()?);
        let run = by_name(name);
        Ok(Callback::rust(name, move |args: &[Value]| {
            let api = me
                .upgrade()
                .ok_or_else(|| speedyspec_wp_hooks::HookError::message("kses is gone"))?;
            let data = args
                .first()
                .and_then(|v| php_string(&v.read()))
                .unwrap_or_default();
            run(&api, &data)
                .map(|out| bytes_value(&out))
                .map_err(|e| speedyspec_wp_hooks::HookError::message(e.to_string()))
        }))
    }

    fn add(&self, hook: &str, callback: Callback, priority: i64) -> Result<(), KsesError> {
        self.d
            .hooks
            .add_filter(&HookName::from(hook), callback, &Value::Int(priority), 1)
            .map_err(hook_error)?;
        Ok(())
    }

    fn remove(&self, hook: &str, name: &str, priority: i64) -> Result<(), KsesError> {
        self.d
            .hooks
            .remove_filter(
                &HookName::from(hook),
                CallbackId(Some(name.to_string())),
                &Value::Int(priority),
            )
            .map_err(hook_error)?;
        Ok(())
    }

    /// `kses_init_filters()`: kses on the title, comment, content and excerpt save filters.
    pub fn kses_init_filters(&self) -> Result<(), KsesError> {
        self.add("title_save_pre", self.native("wp_filter_kses")?, 10)?;
        if self.d.policy.can_unfiltered_html()? {
            self.add(
                "pre_comment_content",
                self.native("wp_filter_post_kses")?,
                10,
            )?;
        } else {
            self.add("pre_comment_content", self.native("wp_filter_kses")?, 10)?;
        }
        for hook in ["content_save_pre", "content_filtered_save_pre"] {
            if let Some(callback) = self
                .d
                .callbacks
                .named_filter("wp_filter_global_styles_post")
            {
                self.add(hook, callback, 9)?;
            }
        }
        for hook in [
            "content_save_pre",
            "excerpt_save_pre",
            "content_filtered_save_pre",
        ] {
            self.add(hook, self.native("wp_filter_post_kses")?, 10)?;
        }
        Ok(())
    }

    /// `kses_remove_filters()`.
    pub fn kses_remove_filters(&self) -> Result<(), KsesError> {
        self.remove("title_save_pre", "wp_filter_kses", 10)?;
        self.remove("pre_comment_content", "wp_filter_post_kses", 10)?;
        self.remove("pre_comment_content", "wp_filter_kses", 10)?;
        self.remove("content_save_pre", "wp_filter_global_styles_post", 9)?;
        self.remove(
            "content_filtered_save_pre",
            "wp_filter_global_styles_post",
            9,
        )?;
        self.remove("content_save_pre", "wp_filter_post_kses", 10)?;
        self.remove("excerpt_save_pre", "wp_filter_post_kses", 10)?;
        self.remove("content_filtered_save_pre", "wp_filter_post_kses", 10)
    }

    /// `kses_init()`: kses filters for users who may not post unfiltered HTML.
    pub fn kses_init(&self) -> Result<(), KsesError> {
        self.kses_remove_filters()?;
        if !self.d.policy.can_unfiltered_html()? {
            self.kses_init_filters()?;
        }
        Ok(())
    }
}
