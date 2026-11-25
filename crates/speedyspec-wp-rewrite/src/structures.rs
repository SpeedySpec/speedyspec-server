//! `WP_Rewrite::init()`, the `using_*` checks and the structure getters.

use std::sync::OnceLock;

use regex::Regex;
use speedyspec_router::wordpress::preg_index;
use speedyspec_wp_hooks::{HookName, HookValue};

use crate::{RewriteError, RewriteService, RewriteState};

fn collapse_slashes(value: &str) -> String {
    static SLASHES: OnceLock<Regex> = OnceLock::new();
    SLASHES
        .get_or_init(|| Regex::new("/+").expect("valid"))
        .replace_all(value, "/")
        .into_owned()
}

fn verbose_page_rules(structure: &str) -> bool {
    static VERBOSE: OnceLock<Regex> = OnceLock::new();
    VERBOSE
        .get_or_init(|| Regex::new("^[^%]*%(?:postname|category|tag|author)%").expect("valid"))
        .is_match(structure)
}

impl RewriteService {
    /// `WP_Rewrite::init()`.
    pub fn init(&self, state: &mut RewriteState) -> Result<(), RewriteError> {
        state.extra_rules.clear();
        state.non_wp_rules.clear();
        state.endpoints.clear();
        state.permalink_structure = self
            .deps
            .options
            .get("permalink_structure")?
            .unwrap_or_default();
        state.front = state
            .permalink_structure
            .find('%')
            .map_or(String::new(), |i| state.permalink_structure[..i].into());
        state.root = String::new();
        if self.using_index_permalinks(state) {
            state.root = format!("{}/", state.index);
        }
        state.random_base = self
            .deps
            .options
            .get_string("wp_random_base")?
            .unwrap_or_default();
        state.author_structure = None;
        state.date_structure = None;
        state.page_structure = None;
        state.search_structure = None;
        state.feed_structure = None;
        state.comment_feed_structure = None;
        state.use_trailing_slashes = state.permalink_structure.ends_with('/');
        state.use_verbose_page_rules = verbose_page_rules(&state.permalink_structure);
        Ok(())
    }

    pub fn using_permalinks(&self, state: &RewriteState) -> bool {
        !state.permalink_structure.is_empty()
    }

    /// `preg_match( '#^/*' . $this->index . '#', $this->permalink_structure )`; the index is used as a regex, as in
    /// WordPress.
    pub fn using_index_permalinks(&self, state: &RewriteState) -> bool {
        if state.permalink_structure.is_empty() {
            return false;
        }
        Regex::new(&format!("^/*{}", state.index))
            .is_ok_and(|re| re.is_match(&state.permalink_structure))
    }

    pub fn using_mod_rewrite_permalinks(&self, state: &RewriteState) -> bool {
        self.using_permalinks(state) && !self.using_index_permalinks(state)
    }

    pub fn preg_index(&self, state: &RewriteState, number: usize) -> String {
        preg_index(number, &state.matches)
    }

    /// Answers a cached structure, or caches `""` and answers `None` (`false`) under plain permalinks.
    fn cached(
        state: &mut RewriteState,
        field: fn(&mut RewriteState) -> &mut Option<String>,
        make: impl FnOnce(&RewriteState) -> String,
    ) -> Option<String> {
        if let Some(cached) = field(state).clone() {
            return Some(cached);
        }
        if state.permalink_structure.is_empty() {
            *field(state) = Some(String::new());
            return None;
        }
        let made = make(state);
        *field(state) = Some(made.clone());
        Some(made)
    }

    pub fn get_date_permastruct(&self, state: &mut RewriteState) -> Option<String> {
        RewriteService::cached(
            state,
            |s| &mut s.date_structure,
            |s| {
                let endians = [
                    "%year%/%monthnum%/%day%",
                    "%day%/%monthnum%/%year%",
                    "%monthnum%/%day%/%year%",
                ];
                let endian = endians
                    .iter()
                    .find(|e| s.permalink_structure.contains(*e))
                    .copied()
                    .unwrap_or("%year%/%monthnum%/%day%");
                let mut front = s.front.clone();
                static TOKEN: OnceLock<Regex> = OnceLock::new();
                let token = TOKEN.get_or_init(|| Regex::new("%.+?%").expect("valid"));
                for (i, m) in token.find_iter(&s.permalink_structure).enumerate() {
                    if m.as_str() == "%post_id%" && i < 3 {
                        front.push_str("date/");
                        break;
                    }
                }
                front + endian
            },
        )
    }

    pub fn get_year_permastruct(&self, state: &mut RewriteState) -> Option<String> {
        let structure = self.get_date_permastruct(state).filter(|s| !s.is_empty())?;
        Some(collapse_slashes(
            &structure.replace("%monthnum%", "").replace("%day%", ""),
        ))
    }

    pub fn get_month_permastruct(&self, state: &mut RewriteState) -> Option<String> {
        let structure = self.get_date_permastruct(state).filter(|s| !s.is_empty())?;
        Some(collapse_slashes(&structure.replace("%day%", "")))
    }

    pub fn get_day_permastruct(&self, state: &mut RewriteState) -> Option<String> {
        self.get_date_permastruct(state)
    }

    pub fn get_category_permastruct(&self, state: &RewriteState) -> Option<String> {
        self.get_extra_permastruct(state, "category")
    }

    pub fn get_tag_permastruct(&self, state: &RewriteState) -> Option<String> {
        self.get_extra_permastruct(state, "post_tag")
    }

    pub fn get_extra_permastruct(&self, state: &RewriteState, name: &str) -> Option<String> {
        if state.permalink_structure.is_empty() {
            return None;
        }
        state
            .extra_permastructs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.struct_.clone())
    }

    pub fn get_author_permastruct(&self, state: &mut RewriteState) -> Option<String> {
        RewriteService::cached(
            state,
            |s| &mut s.author_structure,
            |s| format!("{}{}/%author%", s.front, s.author_base),
        )
    }

    pub fn get_search_permastruct(&self, state: &mut RewriteState) -> Option<String> {
        RewriteService::cached(
            state,
            |s| &mut s.search_structure,
            |s| format!("{}{}/%search%", s.root, s.search_base),
        )
    }

    pub fn get_page_permastruct(&self, state: &mut RewriteState) -> Option<String> {
        RewriteService::cached(
            state,
            |s| &mut s.page_structure,
            |s| format!("{}%pagename%", s.root),
        )
    }

    pub fn get_feed_permastruct(&self, state: &mut RewriteState) -> Option<String> {
        RewriteService::cached(
            state,
            |s| &mut s.feed_structure,
            |s| format!("{}{}/%feed%", s.root, s.feed_base),
        )
    }

    pub fn get_comment_feed_permastruct(&self, state: &mut RewriteState) -> Option<String> {
        RewriteService::cached(
            state,
            |s| &mut s.comment_feed_structure,
            |s| format!("{}{}/{}/%feed%", s.root, s.comments_base, s.feed_base),
        )
    }

    /// `WP_Rewrite::set_permalink_structure()`.
    pub fn set_permalink_structure(
        &self,
        state: &mut RewriteState,
        structure: &str,
    ) -> Result<(), RewriteError> {
        if state.permalink_structure == structure {
            return Ok(());
        }
        let old = state.permalink_structure.clone();
        self.deps.options.update("permalink_structure", structure)?;
        self.init(state)?;
        self.deps.hooks.do_action(
            &HookName::from("permalink_structure_changed"),
            vec![HookValue::Str(old), HookValue::Str(structure.into())],
        )?;
        Ok(())
    }

    pub fn set_category_base(
        &self,
        state: &mut RewriteState,
        base: &str,
    ) -> Result<(), RewriteError> {
        if self.deps.options.get("category_base")?.as_deref() != Some(base) {
            self.deps.options.update("category_base", base)?;
            self.init(state)?;
        }
        Ok(())
    }

    pub fn set_tag_base(&self, state: &mut RewriteState, base: &str) -> Result<(), RewriteError> {
        if self.deps.options.get("tag_base")?.as_deref() != Some(base) {
            self.deps.options.update("tag_base", base)?;
            self.init(state)?;
        }
        Ok(())
    }

    /// `WP_Rewrite::set_random_base()`: the comparison is strict, so an option that is not a string is replaced.
    pub fn set_random_base(
        &self,
        state: &mut RewriteState,
        base: &str,
    ) -> Result<(), RewriteError> {
        if self.deps.options.get_string("wp_random_base")?.as_deref() != Some(base) {
            self.deps.options.update("wp_random_base", base)?;
            self.init(state)?;
        }
        Ok(())
    }

    /// `WP_Rewrite::get_random_base()`: the custom base, else the default translated into the site language, else
    /// `random`.
    pub fn get_random_base(&self, state: &RewriteState) -> String {
        if !state.random_base.is_empty() {
            return state.random_base.clone();
        }
        let translated = self.deps.translations.random_base();
        if translated.is_empty() {
            "random".into()
        } else {
            translated
        }
    }

    /// `WP_Rewrite::post_permalinks_share_random_base()`: whether the permalink structure is only the post name at
    /// the root, so a post can sit at the random base.
    pub fn post_permalinks_share_random_base(&self, state: &RewriteState) -> bool {
        if !self.using_permalinks(state) {
            return false;
        }
        // PCRE's `$` also matches before a final newline.
        let pattern = format!(
            "^{}%postname%/?\\n?$",
            regex::escape(&format!("/{}", state.root))
        );
        Regex::new(&pattern).is_ok_and(|re| re.is_match(&state.permalink_structure))
    }
}
