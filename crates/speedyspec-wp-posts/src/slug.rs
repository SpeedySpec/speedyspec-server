//! `wp_unique_post_slug()` (post.php, 7.2-RC2 lines 5801-5972) between its two filters: the attachment, hierarchical
//! and flat branches, each one owner query (Repository) then the `wp_unique_post_slug_is_bad_*` filter (Strategy,
//! [`BadSlugs`]), and the numbered suffix loop, which cuts the slug through [`SlugTruncation`] (the text seam's
//! `wp_truncate_slug()`).
//!
//! [`unique_post_slug_filtered`] adds the two filters around it (Observer, through [`HookApi`]):
//! [`slug_needs_no_check`] is the test before `pre_wp_unique_post_slug`, and [`unique_post_slug`] the value passed to
//! `wp_unique_post_slug`.

use speedyspec_php_serialize::PhpValue;
use speedyspec_php_serialize::semantics::{bytes_truthy, to_php_int};
use speedyspec_wp_hooks_contracts::{HookApi, HookName, HookValue};
use speedyspec_wp_posts_contracts::{
    BadSlugs, PostError, PostId, PostReader, PostRepository, PostTypeName, PostTypes, Slug,
    SlugTruncation,
};

use crate::hook::{bytes_value, filter_hook, pv};
use crate::is_post_type_hierarchical;

/// `wp_unique_post_slug()`'s first test, before `pre_wp_unique_post_slug`: `draft`, `pending`, `auto-draft`, an
/// `inherit` revision, or a `user_request` need no unique slug.
pub fn slug_needs_no_check(status: &str, post_type: &str) -> bool {
    matches!(status, "draft" | "pending" | "auto-draft")
        || ("inherit" == status && "revision" == post_type)
        || "user_request" == post_type
}

/// `wp_unique_post_slug()`'s arguments.
pub struct SlugRequest<'a> {
    /// `$slug`: the desired `post_name`.
    pub slug: &'a [u8],
    /// `$post_id`: the post's own ID, which the owner queries skip (0 for a new post).
    pub post_id: PostId,
    /// `$post_status`.
    pub status: &'a str,
    /// `$post_type`.
    pub post_type: &'a str,
    /// `$post_parent`: hierarchical slugs are unique among siblings.
    pub parent: PostId,
}

/// `$wp_rewrite->feeds` (an empty list when not an array), its `pagination_base`, and the `permalink_structure`
/// option.
pub struct SlugContext {
    /// `$wp_rewrite->feeds`: `feed`, `rdf`, `rss`, `rss2` and `atom` by default.
    pub feeds: Vec<Vec<u8>>,
    /// `$wp_rewrite->pagination_base`: `page` by default. Matched literally: PHP puts it into the pattern unescaped,
    /// so a base with PCRE metacharacters acts as a pattern there, and one with `@` makes `preg_match` fail on the
    /// delimiter, warn and return false (no conflict). This port diverges for such bases.
    pub pagination_base: Vec<u8>,
    /// `get_option( 'permalink_structure' )`.
    pub permalink_structure: Vec<u8>,
}

/// The ports [`unique_post_slug`] reads through.
pub struct SlugPorts<'a> {
    /// The owner queries.
    pub repository: &'a dyn PostRepository,
    /// `get_post( $post_id )`, which the flat branch makes for the date-archive test.
    pub posts: &'a dyn PostReader,
    /// `is_post_type_hierarchical()`.
    pub types: &'a dyn PostTypes,
    /// `wp_truncate_slug()`.
    pub truncation: &'a dyn SlugTruncation,
    /// The `wp_unique_post_slug_is_bad_*` filters.
    pub bad: &'a dyn BadSlugs,
}

/// [`BadSlugs`] answering false, as WordPress does with no callback.
pub struct NoBadSlugs;

impl BadSlugs for NoBadSlugs {
    fn bad_attachment_slug(&self, _slug: &[u8]) -> bool {
        false
    }

    fn bad_hierarchical_slug(&self, _slug: &[u8], _post_type: &str, _parent: PostId) -> bool {
        false
    }

    fn bad_flat_slug(&self, _slug: &[u8], _post_type: &str) -> bool {
        false
    }
}

/// The owner query of one branch: `$check_sql` with its other arguments bound.
enum OwnerQuery<'a> {
    Attachment,
    Hierarchical(&'a PostTypeName),
    Flat(&'a PostTypeName),
}

/// PHP truth of `$post_name_check`: a missing owner (null) and an owner named `""` or `"0"` are false, so they never
/// force a suffix.
fn truthy(owner: Option<Slug>) -> bool {
    owner.is_some_and(|name| bytes_truthy(name.as_bytes()))
}

impl OwnerQuery<'_> {
    /// Runs the query for `slug` and tests the owner's name for PHP truth.
    fn taken(
        &self,
        slug: &[u8],
        request: &SlugRequest<'_>,
        ports: &SlugPorts<'_>,
    ) -> Result<bool, PostError> {
        let slug = Slug::new(slug);
        let owner = match self {
            OwnerQuery::Attachment => ports
                .repository
                .attachment_slug_owner(&slug, request.post_id),
            OwnerQuery::Hierarchical(post_type) => ports.repository.hierarchical_slug_owner(
                &slug,
                post_type,
                request.post_id,
                request.parent,
            ),
            OwnerQuery::Flat(post_type) => {
                ports
                    .repository
                    .flat_slug_owner(&slug, post_type, request.post_id)
            }
        }?;
        Ok(truthy(owner))
    }
}

/// The body before PCRE's `$`, which matches at the end or before one final `\n`: the bytes, or the bytes less that
/// newline. Both are tried, since `$` may match at either.
fn dollar_bodies(subject: &[u8]) -> impl Iterator<Item = &[u8]> {
    let trimmed = subject.strip_suffix(b"\n");
    std::iter::once(subject).chain(trimmed)
}

/// One or more ASCII digits (`\d` and `[0-9]` without the `u` modifier).
fn all_digits(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

/// `preg_match( '/^[0-9]+$/', $slug )`.
fn is_number(slug: &[u8]) -> bool {
    dollar_bodies(slug).any(all_digits)
}

/// `preg_match( "@^($wp_rewrite->pagination_base)?\d+$@", $slug )`, the base matched literally.
///
/// A divergence for unusual bases: PHP interpolates the base unescaped, so PCRE metacharacters in it act as a
/// pattern, and an `@` ends the pattern early, so `preg_match` warns and returns false (no conflict). Here every
/// base is matched as bytes.
fn is_pagination(slug: &[u8], base: &[u8]) -> bool {
    dollar_bodies(slug)
        .any(|body| all_digits(body) || body.strip_prefix(base).is_some_and(all_digits))
}

/// `in_array( $slug, $feeds, true ) || 'embed' === $slug`.
fn reserved(slug: &[u8], context: &SlugContext) -> bool {
    context.feeds.iter().any(|feed| feed.as_slice() == slug) || slug == b"embed"
}

/// The flat branch's date-archive test (post.php 5908-5931): a number that would read as a year, a month or a day
/// after the `%postname%` segment of the permalink structure.
fn conflicts_with_date_archive(
    request: &SlugRequest<'_>,
    context: &SlugContext,
    own_name: Option<&Slug>,
) -> bool {
    let slug = request.slug;
    if "post" != request.post_type
        || own_name.is_some_and(|name| name.as_bytes() == slug)
        || !is_number(slug)
    {
        return false;
    }
    let number = to_php_int(&PhpValue::Str(slug.to_vec()));
    if number == 0 {
        return false;
    }
    // array_values( array_filter( explode( '/', … ) ) ): array_filter drops "" and "0".
    let parts: Vec<&[u8]> = context
        .permalink_structure
        .split(|&b| b == b'/')
        .filter(|part| bytes_truthy(part))
        .collect();
    match parts.iter().position(|part| *part == b"%postname%") {
        Some(0) => true,
        Some(index) => match parts[index - 1] {
            b"%year%" => number < 13,
            b"%monthnum%" => number < 32,
            _ => false,
        },
        None => false,
    }
}

/// `wp_unique_post_slug()` between its two filters: the slug unchanged when it is free, else the first free
/// `<slug>-<n>` from 2, the slug cut so the whole stays within 200 bytes. When [`slug_needs_no_check`] holds for
/// the request's status and type, the slug comes back unchanged before any query, as WordPress returns it before
/// `pre_wp_unique_post_slug`.
pub fn unique_post_slug(
    request: &SlugRequest<'_>,
    context: &SlugContext,
    ports: &SlugPorts<'_>,
) -> Result<Vec<u8>, PostError> {
    let slug = request.slug;
    if slug_needs_no_check(request.status, request.post_type) {
        return Ok(slug.to_vec());
    }
    let post_type = PostTypeName::new(request.post_type);
    let (query, conflict) = if "attachment" == request.post_type {
        let query = OwnerQuery::Attachment;
        let taken = query.taken(slug, request, ports)?;
        let bad = ports.bad.bad_attachment_slug(slug);
        (query, taken || reserved(slug, context) || bad)
    } else if is_post_type_hierarchical(ports.types, request.post_type) {
        if "nav_menu_item" == request.post_type {
            return Ok(slug.to_vec());
        }
        let query = OwnerQuery::Hierarchical(&post_type);
        let taken = query.taken(slug, request, ports)?;
        let bad = ports
            .bad
            .bad_hierarchical_slug(slug, request.post_type, request.parent);
        let conflict = taken
            || reserved(slug, context)
            || is_pagination(slug, &context.pagination_base)
            || bad;
        (query, conflict)
    } else {
        let query = OwnerQuery::Flat(&post_type);
        let taken = query.taken(slug, request, ports)?;
        let own = ports.posts.post(request.post_id)?;
        let date = conflicts_with_date_archive(request, context, own.as_ref().map(|p| &p.name));
        let bad = ports.bad.bad_flat_slug(slug, request.post_type);
        (query, taken || reserved(slug, context) || date || bad)
    };
    if !conflict {
        return Ok(slug.to_vec());
    }
    let mut suffix: u64 = 2;
    loop {
        let digits = suffix.to_string();
        let mut alt = ports.truncation.truncate(slug, 200 - (digits.len() + 1));
        alt.push(b'-');
        alt.extend_from_slice(digits.as_bytes());
        if !query.taken(&alt, request, ports)? {
            return Ok(alt);
        }
        suffix += 1;
    }
}

/// `wp_unique_post_slug( $slug, $post_id, $post_status, $post_type, $post_parent )` whole (post.php 5801-5972):
/// - when [`slug_needs_no_check`] holds, the slug as given, with no filter;
/// - `pre_wp_unique_post_slug` with `( null, $slug, $post_id, $post_status, $post_type, $post_parent )`: any answer
///   but null is returned as it is;
/// - a hierarchical `nav_menu_item`: the slug as given, before the second filter, as WordPress returns it;
/// - else [`unique_post_slug`] through `wp_unique_post_slug` with `( $slug, $post_id, $post_status, $post_type,
///   $post_parent, $original_slug )`, whose answer is returned as it is.
///
/// The IDs reach the filters as ints and the names as strings (a slug that is not UTF-8 as bytes), as
/// `wp_insert_post()` passes them.
pub fn unique_post_slug_filtered(
    hooks: &dyn HookApi,
    request: &SlugRequest<'_>,
    context: &SlugContext,
    ports: &SlugPorts<'_>,
) -> Result<PhpValue, PostError> {
    if slug_needs_no_check(request.status, request.post_type) {
        return Ok(PhpValue::Str(request.slug.to_vec()));
    }
    let int = |id: PostId| HookValue::Int(i64::try_from(id.0).unwrap_or(i64::MAX));
    let tail = || {
        vec![
            int(request.post_id),
            HookValue::Str(request.status.to_owned()),
            HookValue::Str(request.post_type.to_owned()),
            int(request.parent),
        ]
    };

    let pre = filter_hook(
        hooks,
        &HookName::from("pre_wp_unique_post_slug"),
        HookValue::Null,
        || {
            let mut args = vec![bytes_value(request.slug)];
            args.extend(tail());
            args
        },
    )?;
    if !matches!(&*pre.read(), HookValue::Null) {
        return pv(&pre);
    }

    let slug = unique_post_slug(request, context, ports)?;
    // A hierarchical `nav_menu_item` returns from inside the branch, before the second filter.
    if "nav_menu_item" == request.post_type
        && is_post_type_hierarchical(ports.types, request.post_type)
    {
        return Ok(PhpValue::Str(slug));
    }
    let out = filter_hook(
        hooks,
        &HookName::from("wp_unique_post_slug"),
        bytes_value(&slug),
        || {
            let mut args = tail();
            args.push(bytes_value(request.slug));
            args
        },
    )?;
    pv(&out)
}

#[cfg(test)]
mod tests {
    use speedyspec_php_serialize::PhpKey;
    use speedyspec_wp_posts_contracts::{Post, PostType};
    use speedyspec_wp_posts_memory::MemoryPostRepository;

    use crate::test_support::Silent;

    use super::*;
    use crate::Uncached;

    struct Types(Vec<(&'static str, bool)>);

    impl PostTypes for Types {
        fn post_type(&self, name: &str) -> Option<PostType> {
            self.0.iter().find(|(n, _)| *n == name).map(|(n, h)| {
                PostType::new(
                    PostTypeName::new(*n),
                    vec![(PhpKey::Str(b"hierarchical".to_vec()), PhpValue::Bool(*h))],
                )
            })
        }

        fn post_types(&self) -> Vec<PostType> {
            Vec::new()
        }
    }

    struct Truncation;

    impl SlugTruncation for Truncation {
        fn truncate(&self, slug: &[u8], length: usize) -> Vec<u8> {
            speedyspec_wp_text::helpers::wp_truncate_slug(
                slug,
                i64::try_from(length).unwrap_or(i64::MAX),
            )
        }
    }

    struct AllBad;

    impl BadSlugs for AllBad {
        fn bad_attachment_slug(&self, _slug: &[u8]) -> bool {
            true
        }

        fn bad_hierarchical_slug(&self, _slug: &[u8], _post_type: &str, _parent: PostId) -> bool {
            true
        }

        fn bad_flat_slug(&self, _slug: &[u8], _post_type: &str) -> bool {
            true
        }
    }

    fn table(posts: &[(&[u8], &str)]) -> MemoryPostRepository {
        let repository = MemoryPostRepository::new();
        for (name, post_type) in posts {
            repository
                .insert(&Post {
                    name: Slug::new(*name),
                    post_type: PostTypeName::new(*post_type),
                    ..Post::default()
                })
                .unwrap();
        }
        repository
    }

    fn context() -> SlugContext {
        SlugContext {
            feeds: vec![b"feed".to_vec(), b"rss2".to_vec()],
            pagination_base: b"page".to_vec(),
            permalink_structure: b"/%postname%/".to_vec(),
        }
    }

    fn unique(
        repository: &MemoryPostRepository,
        types: &Types,
        bad: &dyn BadSlugs,
        slug: &[u8],
        post_type: &str,
    ) -> Vec<u8> {
        let reader = Uncached(repository);
        let ports = SlugPorts {
            repository,
            posts: &reader,
            types,
            truncation: &Truncation,
            bad,
        };
        let request = SlugRequest {
            slug,
            post_id: PostId(0),
            status: "publish",
            post_type,
            parent: PostId(0),
        };
        unique_post_slug(&request, &context(), &ports).unwrap()
    }

    fn types() -> Types {
        Types(vec![("post", false), ("page", true), ("attachment", false)])
    }

    #[test]
    fn zero_named_post_never_forces_a_suffix() {
        let repository = table(&[
            (b"0", "post"),
            (b"", "post"),
            (b"", "page"),
            (b"0", "attachment"),
        ]);
        assert_eq!(
            unique(&repository, &types(), &NoBadSlugs, b"0", "post"),
            b"0"
        );
        assert_eq!(unique(&repository, &types(), &NoBadSlugs, b"", "post"), b"");
        assert_eq!(unique(&repository, &types(), &NoBadSlugs, b"", "page"), b"");
        // A hierarchical "0" is a pagination number, so it takes a suffix whatever its owner.
        assert_eq!(
            unique(&repository, &types(), &NoBadSlugs, b"0", "page"),
            b"0-2"
        );
        assert_eq!(
            unique(&repository, &types(), &NoBadSlugs, b"0", "attachment"),
            b"0"
        );
    }

    #[test]
    fn trailing_dash_is_trimmed_before_the_suffix() {
        let repository = table(&[(b"abc-", "post")]);
        assert_eq!(
            unique(&repository, &types(), &NoBadSlugs, b"abc-", "post"),
            b"abc-2"
        );
    }

    #[test]
    fn hierarchical_nav_menu_item_returns_the_slug() {
        let types = Types(vec![("nav_menu_item", true)]);
        let reader = Uncached(&Silent);
        let ports = SlugPorts {
            repository: &Silent,
            posts: &reader,
            types: &types,
            truncation: &Truncation,
            bad: &AllBad,
        };
        let request = SlugRequest {
            slug: b"feed",
            post_id: PostId(0),
            status: "publish",
            post_type: "nav_menu_item",
            parent: PostId(0),
        };
        assert_eq!(
            unique_post_slug(&request, &context(), &ports).unwrap(),
            b"feed"
        );
    }

    #[test]
    fn bad_slug_port_forces_a_suffix() {
        let repository = table(&[]);
        for post_type in ["attachment", "page", "post"] {
            assert_eq!(
                unique(&repository, &types(), &NoBadSlugs, b"fine", post_type),
                b"fine",
                "{post_type}"
            );
            assert_eq!(
                unique(&repository, &types(), &AllBad, b"fine", post_type),
                b"fine-2",
                "{post_type}"
            );
        }
    }

    #[test]
    fn ten_suffixes_truncate_to_197() {
        let slug = vec![b'b'; 200];
        let mut taken = vec![slug.clone()];
        for n in 2..10 {
            let mut alt = vec![b'b'; 198];
            alt.extend_from_slice(format!("-{n}").as_bytes());
            taken.push(alt);
        }
        let posts: Vec<(&[u8], &str)> =
            taken.iter().map(|name| (name.as_slice(), "post")).collect();
        let repository = table(&posts);
        let mut expected = vec![b'b'; 197];
        expected.extend_from_slice(b"-10");
        let result = unique(&repository, &types(), &NoBadSlugs, &slug, "post");
        assert_eq!(result.len(), 200);
        assert_eq!(result, expected);
    }

    #[test]
    fn pagination_and_dates_follow_pcre_dollar() {
        assert!(is_pagination(b"page2", b"page"));
        assert!(is_pagination(b"2\n", b"page"));
        assert!(!is_pagination(b"2\n\n", b"page"));
        assert!(!is_pagination(b"page", b"page"));
        assert!(is_number(b"5\n"));
        assert!(!is_number(b"\n"));
    }
}
