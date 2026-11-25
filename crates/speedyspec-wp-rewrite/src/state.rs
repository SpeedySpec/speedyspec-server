use speedyspec_routing::{Endpoint, Permastruct, RewriteTag, Rule};

/// `WP_Rewrite`'s public properties, with WordPress's names and defaults.
///
/// The cached structures are `None` while unset (PHP's `unset()`), so the getters can tell "not computed yet" from
/// "computed as empty", as WordPress does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteState {
    pub permalink_structure: String,
    pub use_trailing_slashes: bool,
    pub author_base: String,
    pub author_structure: Option<String>,
    pub date_structure: Option<String>,
    pub page_structure: Option<String>,
    pub search_base: String,
    pub search_structure: Option<String>,
    pub comments_base: String,
    pub pagination_base: String,
    pub comments_pagination_base: String,
    pub feed_base: String,
    pub comment_feed_structure: Option<String>,
    pub feed_structure: Option<String>,
    pub front: String,
    pub root: String,
    pub index: String,
    pub matches: String,
    pub rules: Option<Vec<Rule>>,
    pub extra_rules: Vec<Rule>,
    pub extra_rules_top: Vec<Rule>,
    pub non_wp_rules: Vec<Rule>,
    pub extra_permastructs: Vec<(String, Permastruct)>,
    pub endpoints: Vec<Endpoint>,
    pub use_verbose_rules: bool,
    pub use_verbose_page_rules: bool,
    /// `rewritecode`, `rewritereplace` and `queryreplace`, as one list in their order.
    pub tags: Vec<RewriteTag>,
    pub feeds: Vec<String>,
    /// The custom random content base, from the `wp_random_base` option; empty for the translated default.
    pub random_base: String,
}

fn tag(tag: &str, regex: &str, query: &str) -> RewriteTag {
    RewriteTag {
        tag: tag.into(),
        regex: regex.into(),
        query: query.into(),
    }
}

impl Default for RewriteState {
    fn default() -> RewriteState {
        RewriteState {
            permalink_structure: String::new(),
            use_trailing_slashes: false,
            author_base: "author".into(),
            author_structure: None,
            date_structure: None,
            page_structure: None,
            search_base: "search".into(),
            search_structure: None,
            comments_base: "comments".into(),
            pagination_base: "page".into(),
            comments_pagination_base: "comment-page".into(),
            feed_base: "feed".into(),
            comment_feed_structure: None,
            feed_structure: None,
            front: String::new(),
            root: String::new(),
            index: "index.php".into(),
            matches: String::new(),
            rules: None,
            extra_rules: Vec::new(),
            extra_rules_top: Vec::new(),
            non_wp_rules: Vec::new(),
            extra_permastructs: Vec::new(),
            endpoints: Vec::new(),
            use_verbose_rules: false,
            use_verbose_page_rules: true,
            tags: vec![
                tag("%year%", "([0-9]{4})", "year="),
                tag("%monthnum%", "([0-9]{1,2})", "monthnum="),
                tag("%day%", "([0-9]{1,2})", "day="),
                tag("%hour%", "([0-9]{1,2})", "hour="),
                tag("%minute%", "([0-9]{1,2})", "minute="),
                tag("%second%", "([0-9]{1,2})", "second="),
                tag("%postname%", "([^/]+)", "name="),
                tag("%post_id%", "([0-9]+)", "p="),
                tag("%author%", "([^/]+)", "author_name="),
                tag("%pagename%", "([^/]+?)", "pagename="),
                tag("%search%", "(.+)", "s="),
            ],
            feeds: ["feed", "rdf", "rss", "rss2", "atom"]
                .map(String::from)
                .to_vec(),
            random_base: String::new(),
        }
    }
}
