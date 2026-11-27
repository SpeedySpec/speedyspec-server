//! Every post read function as a use case on the buses (Command / CQRS), handled by [`PostsApi`]. The keys are
//! `SpeedySpec\WpPosts\<Name>Handler`; PHP overrides one by binding the same key.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandHandler, Query, QueryHandler, bind_command_handler, bind_query_handler,
};
use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_posts_contracts::{
    LastPostField, MimeFilter, MimeType, Post, PostError, PostId, PostRef, PostTypeArg,
    PostTypeName, Slug,
};

use crate::read::get::PostObject;
use crate::{
    PostsApi, page_children, page_hierarchy, sanitize_post_field_word, sanitize_post_value,
};

macro_rules! message {
    ($kind:ident $name:ident { $($field:ident : $ty:ty),* } -> $result:ty, |$a:ident, $m:ident| $body:expr) => {
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        message!(@impl $kind $name $result);
        message!(@handle $kind $name $result, $a, $m, $body);
    };
    (@impl command $name:ident $result:ty) => {
        impl Command for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpPosts\\", stringify!($name), "Handler");
        }
    };
    (@impl query $name:ident $result:ty) => {
        impl Query for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpPosts\\", stringify!($name), "Handler");
        }
    };
    (@handle command $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl CommandHandler<$name> for PostsApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
    (@handle query $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl QueryHandler<$name> for PostsApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
}

type R<T> = Result<T, PostError>;

message!(query GetPost { post: PostRef } -> R<Option<Post>>, |a, m| a.get_post(m.post));
message!(query GetPostObject { post: PostRef } -> R<Option<PostObject>>, |a, m| a.post_object(m.post));
message!(query GetPostAncestors { post: PostRef } -> R<Vec<PostId>>, |a, m| a.get_post_ancestors(m.post));
message!(query GetPostField { field: Vec<u8>, post: PostRef, context: String } -> R<PhpValue>,
    |a, m| a.get_post_field(&m.field, m.post, &m.context));
message!(query GetPostMimeType { post: PostRef } -> R<Option<MimeType>>, |a, m| a.get_post_mime_type(m.post));
message!(query GetPostStatus { post: PostRef } -> R<PhpValue>, |a, m| a.get_post_status(m.post));
message!(query GetPostType { post: PostRef } -> R<Option<PostTypeName>>, |a, m| a.get_post_type(m.post));
message!(query CountPostsCacheKey { post_type: String, perm: String } -> R<String>,
    |a, m| a.count_posts_cache_key(&m.post_type, &m.perm));
message!(query WpCountPosts { post_type: String, perm: String } -> R<PhpValue>,
    |a, m| a.wp_count_posts(&m.post_type, &m.perm));
message!(query WpCountAttachments { mime_type: Option<MimeFilter> } -> R<PhpValue>,
    |a, m| a.wp_count_attachments(m.mime_type.as_ref()));
message!(query GetAllPageIds {} -> R<Vec<PhpValue>>, |a, m| a.get_all_page_ids());
message!(query GetPageByPath { path: Vec<u8>, post_type: PostTypeArg } -> R<Option<Post>>,
    |a, m| a.get_page_by_path(&m.path, &m.post_type));
message!(query GetPageUri { post: PostRef } -> R<PhpValue>, |a, m| a.get_page_uri(m.post));
message!(query GetLastPostDate { timezone: String, post_type: String } -> R<PhpValue>,
    |a, m| a.get_lastpostdate(&m.timezone, &m.post_type));
message!(query GetLastPostModified { timezone: String, post_type: String } -> R<PhpValue>,
    |a, m| a.get_lastpostmodified(&m.timezone, &m.post_type));
message!(command UpdatePostCache { posts: Vec<Post> } -> R<()>, |a, m| a.update_post_cache(&m.posts));
message!(command UpdatePostCacheObjects { posts: Vec<PhpValue> } -> R<()>,
    |a, m| a.update_post_cache_objects(m.posts));
message!(command UpdatePostCaches { posts: Vec<Post>, post_type: PhpValue, terms: bool, meta: bool } -> R<()>,
    |a, m| a.update_post_caches(&m.posts, &m.post_type, m.terms, m.meta));
message!(command UpdatePostParentCaches { posts: Vec<Post> } -> R<()>, |a, m| a.update_post_parent_caches(&m.posts));
message!(command CleanPostCache { post: PostRef } -> R<()>, |a, m| a.clean_post_cache(m.post));
message!(command UpdatePostmetaCache { ids: Vec<PostId> } -> R<Option<PhpValue>>,
    |a, m| a.update_postmeta_cache(&m.ids));
message!(query WpGetPostParentId { post: PostRef } -> R<Option<PostId>>, |a, m| a.wp_get_post_parent_id(m.post));
message!(command PrimePostCaches { ids: Vec<PostId>, terms: bool, meta: bool } -> R<()>,
    |a, m| a.prime_post_caches(&m.ids, m.terms, m.meta));
message!(command PrimePostCachesGiven { ids: Vec<PostId>, given: Vec<PhpValue>, terms: bool, meta: bool } -> R<()>,
    |a, m| a.prime_post_caches_given(&m.ids, &m.given, m.terms, m.meta));
message!(command PrimePostParentIdCaches { ids: Vec<PostId> } -> R<()>,
    |a, m| a.prime_post_parent_id_caches(&m.ids));
message!(command WpCacheSetPostsLastChanged {} -> R<()>, |a, m| a.wp_cache_set_posts_last_changed());
message!(query GetLastPostTime { timezone: String, field: LastPostField, post_type: String } -> R<PhpValue>,
    |a, m| a.last_post_time(&m.timezone, m.field, &m.post_type));
message!(command UpdatePostCachesObjects { posts: Vec<PhpValue>, post_type: PhpValue, terms: bool, meta: bool } -> R<()>,
    |a, m| a.update_post_caches_objects(m.posts, &m.post_type, m.terms, m.meta));
message!(query SanitizePost { post: PhpValue, context: String } -> R<PhpValue>,
    |a, m| sanitize_post_value(&a.sanitize_ports(), m.post, &m.context));
message!(query SanitizePostField { field: Vec<u8>, value: PhpValue, post_id: i64, context: String } -> R<PhpValue>,
    |a, m| sanitize_post_field_word(&a.sanitize_ports(), &m.field, m.value, m.post_id, &m.context));
message!(query GetPageChildren { page_id: PostId, pages: Vec<Post> } -> R<Vec<Post>>,
    |a, m| Ok(page_children(m.page_id, &m.pages)));
message!(query GetPageHierarchy { pages: Vec<Post>, page_id: PostId } -> R<Vec<(PostId, Slug)>>,
    |a, m| Ok(page_hierarchy(&m.pages, m.page_id)));
message!(query GetPung { post: PostRef } -> R<PhpValue>, |a, m| a.get_pung(m.post));
message!(query GetToPing { post: PostRef } -> R<PhpValue>, |a, m| a.get_to_ping(m.post));
message!(query GetEnclosed { post_id: PhpValue } -> R<PhpValue>, |a, m| a.get_enclosed(&m.post_id));
message!(query IsSticky { post_id: PhpValue } -> R<PhpValue>, |a, m| a.is_sticky(&m.post_id));
message!(query PageTraverseName { page_id: PhpValue, children: PhpValue, result: PhpValue } -> R<PhpValue>,
    |a, m| a.page_traverse_name(&m.page_id, &m.children, m.result));
message!(query GetAvailablePostMimeTypes { post_type: String } -> R<Vec<PhpValue>>,
    |a, m| a.get_available_post_mime_types(&m.post_type));

macro_rules! bind_all {
    ($container:expr, $api:expr; commands: $($c:ty),*; queries: $($q:ty),*) => {
        $( { let a = $api.clone(); bind_command_handler::<$c>($container, move |_, _| Ok(a.clone())); } )*
        $( { let a = $api.clone(); bind_query_handler::<$q>($container, move |_, _| Ok(a.clone())); } )*
    };
}

/// Binds `api` as the handler of every post read message, in `post.php` order; PHP overrides one by binding the
/// same key.
pub fn register_posts_handlers(container: &Container, api: Rc<PostsApi>) {
    bind_all!(container, api;
        commands: UpdatePostCache, UpdatePostCacheObjects, CleanPostCache, UpdatePostCaches, UpdatePostCachesObjects,
            UpdatePostParentCaches, UpdatePostmetaCache, PrimePostCaches, PrimePostCachesGiven, PrimePostParentIdCaches,
            WpCacheSetPostsLastChanged;
        queries: GetPost, GetPostObject, GetPostAncestors, GetPostField, GetPostMimeType, GetPostStatus, GetPostType,
            SanitizePost, SanitizePostField, CountPostsCacheKey, WpCountPosts, WpCountAttachments, GetAllPageIds,
            GetPageByPath, GetPageChildren, GetPageHierarchy, GetPageUri, GetLastPostDate, GetLastPostModified,
            GetLastPostTime, WpGetPostParentId, GetAvailablePostMimeTypes, GetPung, GetToPing, GetEnclosed,
            IsSticky, PageTraverseName);
}
