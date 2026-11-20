//! Every `wpdb` method as a use case: a command or a query handled by [`WpDb`] (Command / CQRS).
//!
//! [`register_wpdb_handlers`] binds [`WpDb`] under each message's `KEY`; PHP overrides one by binding the same key.
//! The readers take no SQL: a `get_*` call with SQL is the `Query` command followed by the reader.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandHandler, Query as CqrsQuery, QueryHandler, bind_command_handler,
    bind_query_handler,
};

use crate::{
    ColLength, OutputShape, QueryOutcome, Shaped, TableScope, WpDb, WpDbError, WpRow, WpValue,
};

macro_rules! message {
    (command $name:ident { $($field:ident : $ty:ty),* $(,)? } -> $result:ty, |$db:ident, $m:ident| $body:expr) => {
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        impl Command for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpDb\\", stringify!($name), "Handler");
        }
        impl CommandHandler<$name> for WpDb {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $db = self;
                $body
            }
        }
    };
    (query $name:ident { $($field:ident : $ty:ty),* $(,)? } -> $result:ty, |$db:ident, $m:ident| $body:expr) => {
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        impl CqrsQuery for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpDb\\", stringify!($name), "Handler");
        }
        impl QueryHandler<$name> for WpDb {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $db = self;
                $body
            }
        }
    };
}

type Data = Vec<(String, WpValue)>;

message!(command Query { sql: Vec<u8> } -> Result<QueryOutcome, WpDbError>, |db, m| db.query_bytes(&m.sql));
message!(command Insert { table: String, data: Data, format: Option<Vec<String>> } -> Result<u64, WpDbError>,
    |db, m| db.insert(&m.table, &m.data, m.format.as_deref()));
message!(command Replace { table: String, data: Data, format: Option<Vec<String>> } -> Result<u64, WpDbError>,
    |db, m| db.replace(&m.table, &m.data, m.format.as_deref()));
message!(command Update { table: String, data: Data, conditions: Data, format: Option<Vec<String>>, where_format: Option<Vec<String>> }
    -> Result<u64, WpDbError>,
    |db, m| db.update(&m.table, &m.data, &m.conditions, m.format.as_deref(), m.where_format.as_deref()));
message!(command Delete { table: String, conditions: Data, where_format: Option<Vec<String>> } -> Result<u64, WpDbError>,
    |db, m| db.delete(&m.table, &m.conditions, m.where_format.as_deref()));
message!(command SetPrefix { prefix: String, set_table_names: bool } -> Result<String, WpDbError>,
    |db, m| db.set_prefix(&m.prefix, m.set_table_names));
message!(command SetBlogId { blog_id: i64, network_id: i64 } -> i64, |db, m| db.set_blog_id(m.blog_id, m.network_id));
message!(command SetCharset { charset: Option<String>, collate: Option<String> } -> Result<(), WpDbError>,
    |db, m| db.set_charset(m.charset.as_deref(), m.collate.as_deref()));
message!(command SetSqlMode { modes: Vec<String> } -> Result<(), WpDbError>, |db, m| db.set_sql_mode(&m.modes));
message!(command Select { db: String } -> Result<(), WpDbError>, |api, m| api.select(&m.db));
message!(command Flush {} -> (), |db, m| db.flush());
message!(command Close {} -> bool, |db, m| db.close());
message!(command CheckConnection { allow_bail: bool } -> Result<bool, WpDbError>, |db, m| db.check_connection(m.allow_bail));

message!(query PrepareSql { query: Option<Vec<u8>>, args: Vec<WpValue> } -> Option<Vec<u8>>,
    |db, m| db.prepare(m.query.as_deref(), &m.args));
message!(query EscLike { text: Vec<u8> } -> Vec<u8>, |db, m| db.esc_like(&m.text));
message!(query Escape { data: WpValue } -> WpValue, |db, m| db.escape(&m.data));
message!(query GetVar { x: usize, y: usize } -> Option<Vec<u8>>, |db, m| db.get_var(m.x, m.y));
message!(query GetRow { output: Option<OutputShape>, y: usize } -> Option<WpRow>, |db, m| db.get_row(m.output, m.y));
message!(query GetCol { x: usize } -> Vec<Option<Vec<u8>>>, |db, m| db.get_col(m.x));
message!(query GetResults { output: Option<OutputShape> } -> Option<Shaped>, |db, m| db.get_results(m.output));
message!(query GetColInfo { info_type: String, col_offset: i64 } -> WpValue, |db, m| db.get_col_info(&m.info_type, m.col_offset));
message!(query GetTableCharset { table: String } -> Result<Option<String>, WpDbError>, |db, m| db.get_table_charset(&m.table));
message!(query GetColCharset { table: String, column: String } -> Result<Option<String>, WpDbError>,
    |db, m| db.get_col_charset(&m.table, &m.column));
message!(query GetColLength { table: String, column: String } -> Result<Option<ColLength>, WpDbError>,
    |db, m| db.get_col_length(&m.table, &m.column));
message!(query GetBlogPrefix { blog_id: Option<i64> } -> String, |db, m| db.get_blog_prefix(m.blog_id));
message!(query Tables { scope: String, prefix: bool, blog_id: i64 } -> Vec<(String, String)>,
    |db, m| db.tables(&m.scope, m.prefix, m.blog_id));
message!(query HasCap { cap: String } -> bool, |db, m| db.has_cap(&m.cap));
message!(query DbVersion {} -> Result<String, WpDbError>, |db, m| db.db_version());
message!(query DbServerInfo {} -> Result<String, WpDbError>, |db, m| db.db_server_info());
message!(query SupportsCollation {} -> bool, |db, m| db.supports_collation());
message!(query GetCharsetCollate {} -> String, |db, m| db.get_charset_collate());

impl WpDb {
    /// `wpdb::set_prefix`.
    pub fn set_prefix(&self, prefix: &str, set_table_names: bool) -> Result<String, WpDbError> {
        let env = self.table_env();
        self.with_tables_mut(|t| t.set_prefix(prefix, set_table_names, &env))
    }

    /// `wpdb::set_blog_id`.
    pub fn set_blog_id(&self, blog_id: i64, network_id: i64) -> i64 {
        let env = self.table_env();
        self.with_tables_mut(|t| t.set_blog_id(blog_id, network_id, &env))
    }

    /// `wpdb::get_blog_prefix`.
    pub fn get_blog_prefix(&self, blog_id: Option<i64>) -> String {
        let env = self.table_env();
        self.with_tables(|t| t.get_blog_prefix(blog_id, &env))
    }

    /// `wpdb::tables`; an unknown scope gives no tables.
    pub fn tables(&self, scope: &str, prefix: bool, blog_id: i64) -> Vec<(String, String)> {
        let Some(scope) = TableScope::parse(scope) else {
            return Vec::new();
        };
        let env = self.table_env();
        self.with_tables(|t| t.tables(scope, prefix, blog_id, &env))
    }
}

macro_rules! bind_all {
    ($container:expr, $api:expr; commands: $($c:ty),*; queries: $($q:ty),*) => {
        $( { let a = $api.clone(); bind_command_handler::<$c>($container, move |_, _| Ok(a.clone())); } )*
        $( { let a = $api.clone(); bind_query_handler::<$q>($container, move |_, _| Ok(a.clone())); } )*
    };
}

/// Binds `api` as the handler of every `wpdb` message.
pub fn register_wpdb_handlers(container: &Container, api: Rc<WpDb>) {
    bind_all!(container, api;
        commands: Query, Insert, Replace, Update, Delete, SetPrefix, SetBlogId, SetCharset, SetSqlMode, Select, Flush,
            Close, CheckConnection;
        queries: PrepareSql, EscLike, Escape, GetVar, GetRow, GetCol, GetResults, GetColInfo, GetTableCharset,
            GetColCharset, GetColLength, GetBlogPrefix, Tables, HasCap, DbVersion, DbServerInfo, SupportsCollation,
            GetCharsetCollate);
}
