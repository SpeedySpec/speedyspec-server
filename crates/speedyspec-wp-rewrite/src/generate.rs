//! `WP_Rewrite::rewrite_rules()`, `page_rewrite_rules()` and `page_uri_index()`.

use speedyspec_router::wordpress::{ExpandContext, PhpArray, generate_rewrite_rules};
use speedyspec_routing::{EndpointMask, PermastructArgs, Rule};
use speedyspec_wp_hooks::{HookName, HookValue};

use crate::ports::{rules_from_value, rules_to_value};
use crate::{RewriteError, RewriteService, RewriteState, RewriteStateSource, UriIndex};

/// The path of a URL, as `parse_url( $url )['path']`; `None` when it has none.
pub(crate) fn url_path(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    rest.find('/').map(|i| rest[i..].to_string())
}

impl RewriteService {
    pub(crate) fn expand(
        &self,
        state: &RewriteState,
        structure: &str,
        args: PermastructArgs,
    ) -> Result<PhpArray, RewriteError> {
        let page_on_front = self.deps.options.get("page_on_front")?.unwrap_or_default();
        let post_types = self.deps.post_types.custom_post_types();
        let ctx = ExpandContext {
            tags: &state.tags,
            endpoints: &state.endpoints,
            feeds: &state.feeds,
            index: &state.index,
            feed_base: &state.feed_base,
            pagination_base: &state.pagination_base,
            comments_pagination_base: &state.comments_pagination_base,
            matches: &state.matches,
            page_on_front: &page_on_front,
            custom_post_types: &post_types,
        };
        Ok(PhpArray::from_rules(&generate_rewrite_rules(
            structure, &args, &ctx,
        )))
    }

    fn args(mask: EndpointMask) -> PermastructArgs {
        PermastructArgs {
            ep_mask: mask,
            ..PermastructArgs::default()
        }
    }

    /// Runs a rules filter; a non-array result becomes empty, as `array_merge` would refuse it.
    fn filter_rules(&self, hook: &str, rules: PhpArray) -> Result<PhpArray, RewriteError> {
        let value = rules_to_value(&rules.into_rules());
        let filtered = self
            .deps
            .hooks
            .apply_filters(&HookName::from(hook), value, vec![])?;
        Ok(PhpArray::from_rules(
            &rules_from_value(&filtered.read()).unwrap_or_default(),
        ))
    }

    /// `WP_Rewrite::page_uri_index()`.
    pub fn page_uri_index(&self) -> (UriIndex, UriIndex) {
        self.deps.pages.page_uris()
    }

    /// `WP_Rewrite::page_rewrite_rules()`: registers `%pagename%` as `(.?.+?)` and expands the page structure.
    pub fn page_rewrite_rules(&self, state: &mut RewriteState) -> Result<Vec<Rule>, RewriteError> {
        self.add_rewrite_tag(state, "%pagename%", "(.?.+?)", "pagename=");
        let structure = self.get_page_permastruct(state).unwrap_or_default();
        let args = PermastructArgs {
            walk_dirs: false,
            ..RewriteService::args(EndpointMask::PAGES)
        };
        Ok(self.expand(state, &structure, args)?.into_rules())
    }

    /// `WP_Rewrite::rewrite_rules()`: every rule, in WordPress's order, through every filter.
    pub fn rewrite_rules(
        &self,
        state: &mut RewriteState,
        source: &dyn RewriteStateSource,
    ) -> Result<Vec<Rule>, RewriteError> {
        if state.permalink_structure.is_empty() {
            return Ok(Vec::new());
        }
        let index = state.index.clone();
        let at_root = matches!(
            url_path(&self.deps.urls.home_url()).as_deref(),
            None | Some("/")
        );
        let root_only = |pattern: &str, query: &str| {
            if at_root {
                PhpArray::from_pairs(vec![(pattern.into(), format!("{index}?{query}"))])
            } else {
                PhpArray::new()
            }
        };
        let robots = root_only("robots\\.txt$", "robots=1");
        let favicon = root_only("favicon\\.ico$", "favicon=1");
        let sitemap = root_only("sitemap\\.xml", "sitemap=index");
        let deprecated_files = PhpArray::from_pairs(vec![
            (
                ".*wp-(atom|rdf|rss|rss2|feed|commentsrss2)\\.php$".into(),
                format!("{index}?feed=old"),
            ),
            (".*wp-app\\.php(/.*)?$".into(), format!("{index}?error=403")),
        ]);
        let mut registration = PhpArray::new();
        if self.deps.multisite.is_multisite_main_site() {
            registration.set(".*wp-signup.php$".into(), format!("{index}?signup=true"));
            registration.set(
                ".*wp-activate.php$".into(),
                format!("{index}?activate=true"),
            );
        }
        registration.set(
            ".*wp-register.php$".into(),
            format!("{index}?register=true"),
        );

        let structure = state.permalink_structure.clone();
        let post = self.expand(
            state,
            &structure,
            RewriteService::args(EndpointMask::PERMALINK),
        )?;
        let post = self.filter_rules("post_rewrite_rules", post)?;
        let date_structure = self.get_date_permastruct(state).unwrap_or_default();
        let date = self.expand(
            state,
            &date_structure,
            RewriteService::args(EndpointMask::DATE),
        )?;
        let date = self.filter_rules("date_rewrite_rules", date)?;
        let root_structure = format!("{}/", state.root);
        let root = self.expand(
            state,
            &root_structure,
            RewriteService::args(EndpointMask::ROOT),
        )?;
        let root = self.filter_rules("root_rewrite_rules", root)?;
        let comments_structure = format!("{}{}", state.root, state.comments_base);
        let comments_args = PermastructArgs {
            paged: false,
            feed: true,
            forcomments: true,
            walk_dirs: false,
            ..RewriteService::args(EndpointMask::COMMENTS)
        };
        let comments = self.expand(state, &comments_structure, comments_args)?;
        let comments = self.filter_rules("comments_rewrite_rules", comments)?;
        let search_structure = self.get_search_permastruct(state).unwrap_or_default();
        let search = self.expand(
            state,
            &search_structure,
            RewriteService::args(EndpointMask::SEARCH),
        )?;
        let search = self.filter_rules("search_rewrite_rules", search)?;
        let author_structure = self.get_author_permastruct(state).unwrap_or_default();
        let author = self.expand(
            state,
            &author_structure,
            RewriteService::args(EndpointMask::AUTHORS),
        )?;
        let author = self.filter_rules("author_rewrite_rules", author)?;
        let page = PhpArray::from_rules(&self.page_rewrite_rules(state)?);
        let page = self.filter_rules("page_rewrite_rules", page)?;

        for (name, permastruct) in state.extra_permastructs.clone() {
            let rules = self.expand(state, &permastruct.struct_, permastruct.args)?;
            let mut rules = self.filter_rules(&format!("{name}_rewrite_rules"), rules)?;
            if name == "post_tag" {
                let value = rules_to_value(&rules.into_rules());
                let filtered = self.deps.hooks.apply_filters_deprecated(
                    &HookName::from("tag_rewrite_rules"),
                    vec![value],
                    &HookValue::Str("3.1.0".into()),
                    &HookValue::Str("post_tag_rewrite_rules".into()),
                    &HookValue::Str(String::new()),
                )?;
                rules =
                    PhpArray::from_rules(&rules_from_value(&filtered.read()).unwrap_or_default());
            }
            state.extra_rules_top = PhpArray::from_rules(&state.extra_rules_top)
                .merge(rules)
                .into_rules();
        }

        let (first, second) = if state.use_verbose_page_rules {
            (page, post)
        } else {
            (post, page)
        };
        let all = PhpArray::from_rules(&state.extra_rules_top)
            .merge(robots)
            .merge(favicon)
            .merge(sitemap)
            .merge(deprecated_files)
            .merge(registration)
            .merge(root)
            .merge(comments)
            .merge(search)
            .merge(author)
            .merge(date)
            .merge(first)
            .merge(second)
            .merge(PhpArray::from_rules(&state.extra_rules));
        state.rules = Some(all.into_rules());

        source.store(state);
        self.deps.hooks.do_action_ref_array(
            &HookName::from("generate_rewrite_rules"),
            vec![source.hook_value()],
        )?;
        *state = source.load()?;
        state.rules = source.load_rules()?;

        let rules = state.rules.clone().unwrap_or_default();
        let filtered = self
            .filter_rules("rewrite_rules_array", PhpArray::from_rules(&rules))?
            .into_rules();
        state.rules = Some(filtered.clone());
        Ok(filtered)
    }
}
