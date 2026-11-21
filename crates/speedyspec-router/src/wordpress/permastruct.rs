//! `WP_Rewrite::generate_rewrite_rules()` and `generate_rewrite_rule()`, statement for statement (Interpreter over a
//! permalink structure). Comments name WordPress's variables so the two can be compared side by side.

use std::sync::OnceLock;

use regex::Regex;
use speedyspec_routing::{Endpoint, EndpointMask, PermastructArgs, RewriteTag, Rule};

use super::PhpArray;

/// What expansion reads from `WP_Rewrite` and from WordPress besides the structure itself.
#[derive(Debug, Clone, Copy)]
pub struct ExpandContext<'a> {
    /// `rewritecode`, `rewritereplace` and `queryreplace`, as one list.
    pub tags: &'a [RewriteTag],
    pub endpoints: &'a [Endpoint],
    pub feeds: &'a [String],
    pub index: &'a str,
    pub feed_base: &'a str,
    pub pagination_base: &'a str,
    pub comments_pagination_base: &'a str,
    /// `WP_Rewrite::$matches`: `"matches"` gives `$matches[N]`, `""` gives `$N`.
    pub matches: &'a str,
    /// `get_option( 'page_on_front' )`; empty or `"0"` when unset.
    pub page_on_front: &'a str,
    /// `get_post_types( array( '_builtin' => false ) )`, each with whether it is hierarchical.
    pub custom_post_types: &'a [(String, bool)],
}

/// `WP_Rewrite::preg_index()`.
pub fn preg_index(number: usize, matches: &str) -> String {
    if matches.is_empty() {
        format!("${number}")
    } else {
        format!("${matches}[{number}]")
    }
}

/// PHP's `str_replace( array, array, $subject )`: each search replaced in turn over the whole string.
fn str_replace<'a>(subject: &str, pairs: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut out = subject.to_string();
    for (search, replace) in pairs {
        if !search.is_empty() {
            out = out.replace(search, replace);
        }
    }
    out
}

fn tokens(structure: &str) -> Vec<String> {
    static TOKEN: OnceLock<Regex> = OnceLock::new();
    TOKEN
        .get_or_init(|| Regex::new("%.+?%").expect("valid"))
        .find_iter(structure)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// PHP truthiness of a string option.
fn truthy(value: &str) -> bool {
    !value.is_empty() && value != "0"
}

/// `WP_Rewrite::generate_rewrite_rule( $permalink_structure, $walk_dirs )`.
pub fn generate_rewrite_rule(structure: &str, walk_dirs: bool, ctx: &ExpandContext) -> Vec<Rule> {
    let args = PermastructArgs {
        with_front: true,
        ep_mask: EndpointMask::NONE,
        paged: false,
        feed: false,
        forcomments: false,
        walk_dirs,
        endpoints: true,
    };
    generate_rewrite_rules(structure, &args, ctx)
}

/// `WP_Rewrite::generate_rewrite_rules( $permalink_structure, $ep_mask, $paged, $feed, $forcomments, $walk_dirs,
/// $endpoints )`.
pub fn generate_rewrite_rules(
    permalink_structure: &str,
    args: &PermastructArgs,
    ctx: &ExpandContext,
) -> Vec<Rule> {
    let pi = |n: usize| preg_index(n, ctx.matches);
    let ep_mask = args.ep_mask;

    // $feedregex2, $feedregex, $trackbackregex, $pageregex, $commentregex, $embedregex.
    let feedregex2 = format!("({})/?$", ctx.feeds.join("|").trim_matches('|'));
    let feedregex = format!("{}/{feedregex2}", ctx.feed_base);
    let trackbackregex = "trackback/?$";
    let pageregex = format!("{}/?([0-9]{{1,}})/?$", ctx.pagination_base);
    let commentregex = format!("{}-([0-9]{{1,}})/?$", ctx.comments_pagination_base);
    let embedregex = "embed/?$";

    // $ep_query_append: epmatch => ( mask, epquery ), a later endpoint with the same name overwriting.
    let mut ep_query_append: Vec<(String, EndpointMask, String)> = Vec::new();
    if args.endpoints {
        for endpoint in ctx.endpoints {
            let epmatch = format!("{}(/(.*))?/?$", endpoint.name);
            let epquery = format!("&{}=", endpoint.query_var);
            match ep_query_append.iter_mut().find(|(m, _, _)| *m == epmatch) {
                Some(entry) => *entry = (epmatch, endpoint.mask, epquery),
                None => ep_query_append.push((epmatch, endpoint.mask, epquery)),
            }
        }
    }

    // $front: everything up to the first tag.
    let front = permalink_structure
        .find('%')
        .map_or("", |i| &permalink_structure[..i]);
    let all_tokens = tokens(permalink_structure);
    let index = ctx.index;

    // $queries[i]: the query for the first i+1 tags.
    let mut queries: Vec<String> = Vec::with_capacity(all_tokens.len());
    for (i, token) in all_tokens.iter().enumerate() {
        let previous = if i > 0 {
            format!("{}&", queries[i - 1])
        } else {
            String::new()
        };
        let query_token = str_replace(
            token,
            ctx.tags.iter().map(|t| (t.tag.as_str(), t.query.as_str())),
        ) + &pi(i + 1);
        queries.push(previous + &query_token);
    }

    // $structure without the front, trimmed; $dirs.
    let mut structure = permalink_structure.to_string();
    if front != "/" && !front.is_empty() {
        structure = structure.replace(front, "");
    }
    let structure = structure.trim_matches('/').to_string();
    let dirs: Vec<String> = if args.walk_dirs {
        structure.split('/').map(str::to_string).collect()
    } else {
        vec![structure]
    };
    let front = front.trim_start_matches('/');

    let mut post_rewrite = PhpArray::new();
    let mut struct_ = front.to_string();
    for dir in &dirs {
        struct_.push_str(dir);
        struct_.push('/');
        struct_ = struct_.trim_start_matches('/').to_string();

        let mut match_ = str_replace(
            &struct_,
            ctx.tags.iter().map(|t| (t.tag.as_str(), t.regex.as_str())),
        );
        let num_toks = tokens(&struct_).len();
        let query = if num_toks > 0 {
            queries.get(num_toks - 1).cloned().unwrap_or_default()
        } else {
            String::new()
        };

        let ep_mask_specific = match dir.as_str() {
            "%year%" => EndpointMask::YEAR,
            "%monthnum%" => EndpointMask::MONTH,
            "%day%" => EndpointMask::DAY,
            _ => EndpointMask::NONE,
        };

        let pagematch = format!("{match_}{pageregex}");
        let pagequery = format!("{index}?{query}&paged={}", pi(num_toks + 1));
        let commentmatch = format!("{match_}{commentregex}");
        let commentquery = format!("{index}?{query}&cpage={}", pi(num_toks + 1));
        let root_comment = truthy(ctx.page_on_front).then(|| {
            (
                format!("{match_}{commentregex}"),
                format!(
                    "{index}?{query}&page_id={}&cpage={}",
                    ctx.page_on_front,
                    pi(num_toks + 1)
                ),
            )
        });
        let feedmatch = format!("{match_}{feedregex}");
        let mut feedquery = format!("{index}?{query}&feed={}", pi(num_toks + 1));
        let feedmatch2 = format!("{match_}{feedregex2}");
        let mut feedquery2 = format!("{index}?{query}&feed={}", pi(num_toks + 1));
        let embedmatch = format!("{match_}{embedregex}");
        let embedquery = format!("{index}?{query}&embed=true");
        if args.forcomments {
            feedquery.push_str("&withcomments=1");
            feedquery2.push_str("&withcomments=1");
        }

        let mut rewrite = PhpArray::new();
        if args.feed {
            rewrite = PhpArray::from_pairs(vec![
                (feedmatch, feedquery),
                (feedmatch2, feedquery2),
                (embedmatch.clone(), embedquery.clone()),
            ]);
        }
        if args.paged {
            rewrite = rewrite.merge(PhpArray::from_pairs(vec![(pagematch, pagequery)]));
        }
        if ep_mask.intersects(EndpointMask::PAGES) || ep_mask.intersects(EndpointMask::PERMALINK) {
            rewrite = rewrite.merge(PhpArray::from_pairs(vec![(commentmatch, commentquery)]));
        } else if ep_mask.intersects(EndpointMask::ROOT)
            && let Some(root_comment) = root_comment
        {
            rewrite = rewrite.merge(PhpArray::from_pairs(vec![root_comment]));
        }
        if args.endpoints {
            for (regex, mask, epquery) in &ep_query_append {
                if mask.intersects(ep_mask) || mask.intersects(ep_mask_specific) {
                    rewrite.set(
                        format!("{match_}{regex}"),
                        format!("{index}?{query}{epquery}{}", pi(num_toks + 2)),
                    );
                }
            }
        }

        if num_toks > 0 {
            let has = |tag: &str| struct_.contains(tag);
            let mut post = false;
            let mut page = false;
            if has("%postname%")
                || has("%post_id%")
                || has("%pagename%")
                || (has("%year%")
                    && has("%monthnum%")
                    && has("%day%")
                    && has("%hour%")
                    && has("%minute%")
                    && has("%second%"))
            {
                post = true;
                if has("%pagename%") {
                    page = true;
                }
            }
            if !post {
                for (ptype, hierarchical) in ctx.custom_post_types {
                    if has(&format!("%{ptype}%")) {
                        post = true;
                        page = *hierarchical;
                        break;
                    }
                }
            }

            let mut extras = None;
            let final_query;
            if post {
                let trackbackmatch = format!("{match_}{trackbackregex}");
                let trackbackquery = format!("{index}?{query}&tb=1");
                let embedmatch = format!("{match_}{embedregex}");
                let embedquery = format!("{index}?{query}&embed=true");
                match_ = match_.trim_end_matches('/').to_string();
                let submatchbase = match_.replace(['(', ')'], "");

                let mut sub1 = format!("{submatchbase}/([^/]+)/");
                let sub1tb = format!("{sub1}{trackbackregex}");
                let sub1feed = format!("{sub1}{feedregex}");
                let sub1feed2 = format!("{sub1}{feedregex2}");
                let sub1comment = format!("{sub1}{commentregex}");
                let sub1embed = format!("{sub1}{embedregex}");
                let mut sub2 = format!("{submatchbase}/attachment/([^/]+)/");
                let sub2tb = format!("{sub2}{trackbackregex}");
                let sub2feed = format!("{sub2}{feedregex}");
                let sub2feed2 = format!("{sub2}{feedregex2}");
                let sub2comment = format!("{sub2}{commentregex}");
                let sub2embed = format!("{sub2}{embedregex}");

                let subquery = format!("{index}?attachment={}", pi(1));
                let subtbquery = format!("{subquery}&tb=1");
                let subfeedquery = format!("{subquery}&feed={}", pi(2));
                let subcommentquery = format!("{subquery}&cpage={}", pi(2));
                let subembedquery = format!("{subquery}&embed=true");

                if args.endpoints {
                    for (regex, mask, epquery) in &ep_query_append {
                        if mask.intersects(EndpointMask::ATTACHMENT) {
                            rewrite.set(
                                format!("{sub1}{regex}"),
                                format!("{subquery}{epquery}{}", pi(3)),
                            );
                            rewrite.set(
                                format!("{sub2}{regex}"),
                                format!("{subquery}{epquery}{}", pi(3)),
                            );
                        }
                    }
                }

                sub1.push_str("?$");
                sub2.push_str("?$");
                match_ = format!("{match_}(?:/([0-9]+))?/?$");
                final_query = format!("{index}?{query}&page={}", pi(num_toks + 1));
                extras = Some((
                    (trackbackmatch, trackbackquery),
                    (embedmatch, embedquery),
                    page,
                    vec![
                        (sub1, subquery.clone()),
                        (sub1tb, subtbquery.clone()),
                        (sub1feed, subfeedquery.clone()),
                        (sub1feed2, subfeedquery.clone()),
                        (sub1comment, subcommentquery.clone()),
                        (sub1embed, subembedquery.clone()),
                    ],
                    vec![
                        (sub2, subquery),
                        (sub2tb, subtbquery),
                        (sub2feed, subfeedquery.clone()),
                        (sub2feed2, subfeedquery),
                        (sub2comment, subcommentquery),
                        (sub2embed, subembedquery),
                    ],
                ));
            } else {
                match_.push_str("?$");
                final_query = format!("{index}?{query}");
            }

            rewrite = rewrite.merge(PhpArray::from_pairs(vec![(match_.clone(), final_query)]));

            if let Some((trackback, embed, page, sub1, sub2)) = extras {
                rewrite = PhpArray::from_pairs(vec![trackback]).merge(rewrite);
                rewrite = PhpArray::from_pairs(vec![embed]).merge(rewrite);
                if !page {
                    rewrite = rewrite.merge(PhpArray::from_pairs(sub1));
                }
                rewrite = PhpArray::from_pairs(sub2).merge(rewrite);
            }
        }

        post_rewrite = rewrite.merge(post_rewrite);
    }

    post_rewrite.into_rules()
}
