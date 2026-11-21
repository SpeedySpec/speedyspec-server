//! `WP_Rewrite::wp_rewrite_rules()`, `refresh_rewrite_rules()` and `flush_rules()`.

use speedyspec_routing::Rule;
use speedyspec_wp_hooks::{Callback, HookName, HookValue};

use crate::{RewriteError, RewriteService, RewriteState, RewriteStateSource};

/// The callback id of the deferred flush, so registering it twice keeps one, as WordPress's
/// `add_action( 'wp_loaded', array( $this, 'flush_rules' ) )` does.
const DEFERRED_FLUSH: &str = "speedyspec_wp_rewrite_flush_rules";

impl RewriteService {
    /// `WP_Rewrite::wp_rewrite_rules()`: the stored rules, generated and saved when there are none.
    pub fn wp_rewrite_rules(
        &self,
        state: &mut RewriteState,
        source: &dyn RewriteStateSource,
    ) -> Result<Vec<Rule>, RewriteError> {
        state.rules = self.deps.options.rules()?;
        if state.rules.is_none() {
            self.refresh_rewrite_rules(state, source)?;
        }
        Ok(state.rules.clone().unwrap_or_default())
    }

    /// `WP_Rewrite::refresh_rewrite_rules()`: generates the rules and saves them, or, before `wp_loaded`, flushes
    /// on `wp_loaded` instead, since not every plugin has added its rules yet.
    fn refresh_rewrite_rules(
        &self,
        state: &mut RewriteState,
        source: &dyn RewriteStateSource,
    ) -> Result<(), RewriteError> {
        state.rules = None;
        state.matches = "matches".into();
        self.rewrite_rules(state, source)?;
        if !self.deps.loaded.did_wp_loaded() {
            source.store(state);
            self.flush_on_wp_loaded()?;
        } else {
            match &state.rules {
                Some(rules) => self.deps.options.set_rules(rules)?,
                // Plain permalinks: WordPress leaves `$this->rules` as '' and stores that.
                None => self.deps.options.update("rewrite_rules", "")?,
            }
        }
        Ok(())
    }

    /// Registers the flush WordPress defers to `wp_loaded`. It works on the canonical state ([`RewriteDeps::state`],
    /// the `$wp_rewrite` object), since the request's state is gone by then.
    ///
    /// [`RewriteDeps::state`]: crate::RewriteDeps::state
    fn flush_on_wp_loaded(&self) -> Result<(), RewriteError> {
        let service = RewriteService {
            deps: self.deps.clone(),
            do_hard_later: self.do_hard_later.clone(),
        };
        let callback = Callback::rust(DEFERRED_FLUSH, move |_: &[HookValue]| {
            let source = service.deps.state.clone();
            let mut state = source
                .load()
                .map_err(|err| speedyspec_wp_hooks::HookError::message(err.0))?;
            service
                .flush_rules(&mut state, source.as_ref(), true)
                .map_err(|err| speedyspec_wp_hooks::HookError::message(err.0))?;
            source.store(&state);
            Ok(HookValue::Null)
        });
        self.deps.hooks.add_filter(
            &HookName::from("wp_loaded"),
            callback,
            &HookValue::Int(10),
            1,
        )?;
        Ok(())
    }

    /// `WP_Rewrite::flush_rules()`: regenerates and saves the rules, then fires `flush_rewrite_rules_hard` for a
    /// hard flush. SpeedySpec writes no server files, so nothing else follows.
    pub fn flush_rules(
        &self,
        state: &mut RewriteState,
        source: &dyn RewriteStateSource,
        hard: bool,
    ) -> Result<(), RewriteError> {
        if !self.deps.loaded.did_wp_loaded() {
            source.store(state);
            self.flush_on_wp_loaded()?;
            let later = self.do_hard_later.get().map_or(hard, |later| later || hard);
            self.do_hard_later.set(Some(later));
            return Ok(());
        }
        let hard = self.do_hard_later.take().unwrap_or(hard);
        self.refresh_rewrite_rules(state, source)?;
        if !hard {
            return Ok(());
        }
        self.deps.hooks.apply_filters(
            &HookName::from("flush_rewrite_rules_hard"),
            HookValue::Bool(true),
            vec![],
        )?;
        Ok(())
    }
}
