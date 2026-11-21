//! WordPress's routing concepts, as router types.

/// A WordPress rewrite rule: a regex over the request path (anchored at the start when matched) and the query
/// string it maps to, with `$matches[N]` references.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rule {
    pub pattern: String,
    pub query: String,
}

/// Where `add_rewrite_rule` puts a rule: before or after the generated rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RulePriority {
    Top,
    Bottom,
}

/// WordPress's endpoint mask (`EP_*`), with WordPress's bit values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EndpointMask(pub u32);

impl EndpointMask {
    pub const NONE: EndpointMask = EndpointMask(0);
    pub const PERMALINK: EndpointMask = EndpointMask(1);
    pub const ATTACHMENT: EndpointMask = EndpointMask(2);
    pub const DATE: EndpointMask = EndpointMask(4);
    pub const YEAR: EndpointMask = EndpointMask(8);
    pub const MONTH: EndpointMask = EndpointMask(16);
    pub const DAY: EndpointMask = EndpointMask(32);
    pub const ROOT: EndpointMask = EndpointMask(64);
    pub const COMMENTS: EndpointMask = EndpointMask(128);
    pub const SEARCH: EndpointMask = EndpointMask(256);
    pub const CATEGORIES: EndpointMask = EndpointMask(512);
    pub const TAGS: EndpointMask = EndpointMask(1024);
    pub const AUTHORS: EndpointMask = EndpointMask(2048);
    pub const PAGES: EndpointMask = EndpointMask(4096);
    pub const ALL_ARCHIVES: EndpointMask = EndpointMask(3644);
    pub const ALL: EndpointMask = EndpointMask(8191);

    /// True when every bit of `other` is set, as WordPress's `( $mask & EP_X ) === EP_X`.
    pub fn contains(self, other: EndpointMask) -> bool {
        self.0 & other.0 == other.0
    }

    /// True when any bit is shared, as WordPress's `$mask & EP_X`.
    pub fn intersects(self, other: EndpointMask) -> bool {
        self.0 & other.0 != 0
    }
}

/// A rewrite tag: `%postname%` stands for `regex` and maps to `query` (such as `name=`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RewriteTag {
    pub tag: String,
    pub regex: String,
    pub query: String,
}

/// An endpoint added with `add_rewrite_endpoint`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint {
    pub mask: EndpointMask,
    pub name: String,
    /// The query variable it sets; empty when the endpoint registers none.
    pub query_var: String,
}

/// The arguments of `add_permastruct`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PermastructArgs {
    pub with_front: bool,
    pub ep_mask: EndpointMask,
    pub paged: bool,
    pub feed: bool,
    pub forcomments: bool,
    pub walk_dirs: bool,
    pub endpoints: bool,
}

impl Default for PermastructArgs {
    /// WordPress's defaults for `add_permastruct`.
    fn default() -> PermastructArgs {
        PermastructArgs {
            with_front: true,
            ep_mask: EndpointMask::NONE,
            paged: true,
            feed: true,
            forcomments: false,
            walk_dirs: true,
            endpoints: true,
        }
    }
}

/// A permalink structure such as `%year%/%monthnum%/%postname%/`, with its arguments.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Permastruct {
    pub struct_: String,
    pub args: PermastructArgs,
}
