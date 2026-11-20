//! The native functions the `wpdb` facade calls, each taking the instance handle first.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandHandler, HandlerRef, Query as CqrsQuery, QueryHandler, QueryRef,
};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_db::*;

use crate::convert::*;

const NATIVE: &str = "SpeedySpec\\WpDb\\Native\\";

/// The handlers the global instance dispatches through, each resolved once.
pub(crate) struct Handlers {
    query: HandlerRef<Query>,
    insert: HandlerRef<Insert>,
    replace: HandlerRef<Replace>,
    update: HandlerRef<Update>,
    delete: HandlerRef<Delete>,
    set_prefix: HandlerRef<SetPrefix>,
    set_blog_id: HandlerRef<SetBlogId>,
    set_charset: HandlerRef<SetCharset>,
    set_sql_mode: HandlerRef<SetSqlMode>,
    select: HandlerRef<Select>,
    flush: HandlerRef<Flush>,
    close: HandlerRef<Close>,
    check_connection: HandlerRef<CheckConnection>,
    prepare: QueryRef<PrepareSql>,
    esc_like: QueryRef<EscLike>,
    escape: QueryRef<Escape>,
    get_var: QueryRef<GetVar>,
    get_row: QueryRef<GetRow>,
    get_col: QueryRef<GetCol>,
    get_results: QueryRef<GetResults>,
    get_col_info: QueryRef<GetColInfo>,
    get_table_charset: QueryRef<GetTableCharset>,
    get_col_charset: QueryRef<GetColCharset>,
    get_col_length: QueryRef<GetColLength>,
    get_blog_prefix: QueryRef<GetBlogPrefix>,
    tables: QueryRef<Tables>,
    has_cap: QueryRef<HasCap>,
    db_version: QueryRef<DbVersion>,
    db_server_info: QueryRef<DbServerInfo>,
    supports_collation: QueryRef<SupportsCollation>,
    get_charset_collate: QueryRef<GetCharsetCollate>,
}

impl Handlers {
    pub(crate) fn new(c: &Rc<Container>) -> Handlers {
        Handlers {
            query: HandlerRef::new(c.clone()),
            insert: HandlerRef::new(c.clone()),
            replace: HandlerRef::new(c.clone()),
            update: HandlerRef::new(c.clone()),
            delete: HandlerRef::new(c.clone()),
            set_prefix: HandlerRef::new(c.clone()),
            set_blog_id: HandlerRef::new(c.clone()),
            set_charset: HandlerRef::new(c.clone()),
            set_sql_mode: HandlerRef::new(c.clone()),
            select: HandlerRef::new(c.clone()),
            flush: HandlerRef::new(c.clone()),
            close: HandlerRef::new(c.clone()),
            check_connection: HandlerRef::new(c.clone()),
            prepare: QueryRef::new(c.clone()),
            esc_like: QueryRef::new(c.clone()),
            escape: QueryRef::new(c.clone()),
            get_var: QueryRef::new(c.clone()),
            get_row: QueryRef::new(c.clone()),
            get_col: QueryRef::new(c.clone()),
            get_results: QueryRef::new(c.clone()),
            get_col_info: QueryRef::new(c.clone()),
            get_table_charset: QueryRef::new(c.clone()),
            get_col_charset: QueryRef::new(c.clone()),
            get_col_length: QueryRef::new(c.clone()),
            get_blog_prefix: QueryRef::new(c.clone()),
            tables: QueryRef::new(c.clone()),
            has_cap: QueryRef::new(c.clone()),
            db_version: QueryRef::new(c.clone()),
            db_server_info: QueryRef::new(c.clone()),
            supports_collation: QueryRef::new(c.clone()),
            get_charset_collate: QueryRef::new(c.clone()),
        }
    }
}

type R = Result<Value, NativeError>;

/// One call's instance: its `WpDb` and whether it is the global (which dispatches through the handlers).
struct Ctx {
    db: Rc<WpDb>,
    global: bool,
    shared: Option<Rc<crate::Shared>>,
}

impl Ctx {
    fn cmd<C: Command>(
        &self,
        pick: impl Fn(&Handlers) -> &HandlerRef<C>,
        m: C,
    ) -> Result<C::Result, NativeError>
    where
        WpDb: CommandHandler<C>,
    {
        match (&self.shared, self.global) {
            (Some(s), true) => pick(&s.handlers)
                .call(m)
                .map_err(|e| NativeError::throw("Error", e.to_string())),
            _ => Ok(CommandHandler::<C>::handle(&*self.db, m)),
        }
    }

    fn ask<Q: CqrsQuery>(
        &self,
        pick: impl Fn(&Handlers) -> &QueryRef<Q>,
        m: Q,
    ) -> Result<Q::Result, NativeError>
    where
        WpDb: QueryHandler<Q>,
    {
        match (&self.shared, self.global) {
            (Some(s), true) => pick(&s.handlers)
                .call(m)
                .map_err(|e| NativeError::throw("Error", e.to_string())),
            _ => Ok(QueryHandler::<Q>::handle(&*self.db, m)),
        }
    }

    /// A reader's SQL, through the `Query` handler.
    fn run_reader(&self, query: &Value) -> Result<(), NativeError> {
        let sql = if matches!(query, Value::Null) {
            None
        } else {
            bytes(query)
        };
        if let Some(sql) = self.db.reader_query(sql.as_deref()) {
            let _ = self.cmd(|h| &h.query, Query { sql })?;
        }
        Ok(())
    }
}

fn ctx(args: &[Value]) -> Result<Ctx, NativeError> {
    let handle = args.first().map(int).unwrap_or(0);
    let (db, global) = crate::instance(handle)
        .ok_or_else(|| NativeError::throw("Error", "this wpdb instance is closed"))?;
    let shared = crate::SHARED.with(|s| s.borrow().clone());
    Ok(Ctx { db, global, shared })
}

fn a(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

fn opt_str(v: &Value) -> Option<String> {
    (!matches!(v, Value::Null)).then(|| string(v))
}

/// A `WP_Error` the facade makes (or `false` without WordPress's class).
fn wp_error(code: &str, message: &str) -> Value {
    Value::Array(vec![
        (Key::Str("__speedyspec_error".into()), Value::Bool(true)),
        (Key::Str("code".into()), Value::Str(code.into())),
        (Key::Str("message".into()), Value::Str(message.into())),
    ])
}

fn count_result(r: Result<u64, WpDbError>) -> Value {
    r.map_or(Value::Bool(false), |n| {
        Value::Int(i64::try_from(n).unwrap_or(i64::MAX))
    })
}

fn charset_value(r: Result<Option<String>, WpDbError>) -> Value {
    match r {
        Ok(Some(c)) => Value::Str(c),
        Ok(None) => Value::Bool(false),
        Err(e) => wp_error("wpdb_get_table_charset_failure", &e.last_error()),
    }
}

fn row_value(row: &WpRow, shape: OutputShape) -> Value {
    match shape {
        OutputShape::ArrayN => row_list(row),
        _ => row_assoc(row),
    }
}

// ----- the native functions

fn n_open(args: &[Value]) -> R {
    crate::open(
        &string(&a(args, 0)),
        &string(&a(args, 1)),
        &string(&a(args, 2)),
        &string(&a(args, 3)),
    )
    .map(Value::Int)
    .map_err(|e| NativeError::throw("Error", e.to_string()))
}

fn n_construct(args: &[Value]) -> R {
    let c = ctx(args)?;
    let constant = |n: &str| c.db.host().constant(n).is_some_and(|v| truthy(&v));
    if constant("WP_DEBUG") && constant("WP_DEBUG_DISPLAY") {
        c.db.show_errors(true);
    }
    if c.db.host().constant("WP_SETUP_CONFIG").is_none() {
        let _ = c.db.db_connect(true);
    }
    Ok(Value::Null)
}

fn n_query(args: &[Value]) -> R {
    let c = ctx(args)?;
    let Some(sql) = bytes(&a(args, 1)) else {
        return Ok(Value::Bool(false));
    };
    Ok(c.cmd(|h| &h.query, Query { sql })?
        .map_or(Value::Bool(false), |o| o.to_wp()))
}

fn n_prepare(args: &[Value]) -> R {
    let c = ctx(args)?;
    let query = match a(args, 1) {
        Value::Null => None,
        q => bytes(&q),
    };
    let rest: Vec<Value> = args.iter().skip(2).cloned().collect();
    Ok(opt_bytes(
        c.ask(|h| &h.prepare, PrepareSql { query, args: rest })?,
    ))
}

fn n_insert(args: &[Value]) -> R {
    let c = ctx(args)?;
    let Some(data) = data(&a(args, 2)) else {
        return Ok(Value::Bool(false));
    };
    let m = Insert {
        table: string(&a(args, 1)),
        data,
        format: formats(&a(args, 3)),
    };
    Ok(count_result(c.cmd(|h| &h.insert, m)?))
}

fn n_replace(args: &[Value]) -> R {
    let c = ctx(args)?;
    let Some(data) = data(&a(args, 2)) else {
        return Ok(Value::Bool(false));
    };
    let m = Replace {
        table: string(&a(args, 1)),
        data,
        format: formats(&a(args, 3)),
    };
    Ok(count_result(c.cmd(|h| &h.replace, m)?))
}

fn n_insert_replace_helper(args: &[Value]) -> R {
    let c = ctx(args)?;
    let Some(data) = data(&a(args, 2)) else {
        return Ok(Value::Bool(false));
    };
    let verb = match a(args, 4) {
        Value::Null => "INSERT".to_string(),
        v => string(&v),
    };
    Ok(count_result(c.db.insert_replace_helper(
        &string(&a(args, 1)),
        &data,
        formats(&a(args, 3)).as_deref(),
        &verb,
    )))
}

fn n_update(args: &[Value]) -> R {
    let c = ctx(args)?;
    let (Some(data), Some(conditions)) = (data(&a(args, 2)), data(&a(args, 3))) else {
        return Ok(Value::Bool(false));
    };
    let m = Update {
        table: string(&a(args, 1)),
        data,
        conditions,
        format: formats(&a(args, 4)),
        where_format: formats(&a(args, 5)),
    };
    Ok(count_result(c.cmd(|h| &h.update, m)?))
}

fn n_delete(args: &[Value]) -> R {
    let c = ctx(args)?;
    let Some(conditions) = data(&a(args, 2)) else {
        return Ok(Value::Bool(false));
    };
    let m = Delete {
        table: string(&a(args, 1)),
        conditions,
        where_format: formats(&a(args, 3)),
    };
    Ok(count_result(c.cmd(|h| &h.delete, m)?))
}

fn n_get_var(args: &[Value]) -> R {
    let c = ctx(args)?;
    let (query, x, y) = (
        a(args, 1),
        int(&a(args, 2)).max(0) as usize,
        int(&a(args, 3)).max(0) as usize,
    );
    c.db.set_func_call(format!("$db->get_var(\"{}\", {x}, {y})", string(&query)));
    c.run_reader(&query)?;
    Ok(opt_bytes(c.ask(|h| &h.get_var, GetVar { x, y })?))
}

/// `null`, `['o', index]` (the facade hands out that row's object), or the row as an array.
fn n_get_row(args: &[Value]) -> R {
    let c = ctx(args)?;
    let (query, output, y) = (a(args, 1), a(args, 2), int(&a(args, 3)).max(0) as usize);
    c.db.set_func_call(format!(
        "$db->get_row(\"{}\",{},{y})",
        string(&query),
        string(&output)
    ));
    if !truthy(&query) {
        return Ok(Value::Null);
    }
    c.run_reader(&query)?;
    let shape = shape(&output);
    let row = c.ask(|h| &h.get_row, GetRow { output: shape, y })?;
    Ok(match (row, shape) {
        (None, _) | (_, None) => Value::Null,
        (Some(_), Some(OutputShape::Object | OutputShape::ObjectK)) => {
            list(vec![Value::Str("o".into()), Value::Int(y as i64)])
        }
        (Some(row), Some(s)) => row_value(&row, s),
    })
}

fn n_get_col(args: &[Value]) -> R {
    let c = ctx(args)?;
    c.run_reader(&a(args, 1))?;
    let x = int(&a(args, 2)).max(0) as usize;
    Ok(list(
        c.ask(|h| &h.get_col, GetCol { x })?
            .into_iter()
            .map(opt_bytes)
            .collect(),
    ))
}

/// `null`, `['o']` (the rows' objects), `['k', [[key, index], ..]]` (`OBJECT_K`) or `['a', rows]`.
fn n_get_results(args: &[Value]) -> R {
    let c = ctx(args)?;
    let (query, output) = (a(args, 1), a(args, 2));
    c.db.set_func_call(format!(
        "$db->get_results(\"{}\", {})",
        string(&query),
        string(&output)
    ));
    if !truthy(&query) {
        return Ok(Value::Null);
    }
    c.run_reader(&query)?;
    let shape = shape(&output);
    let results = c.ask(|h| &h.get_results, GetResults { output: shape })?;
    Ok(match (results, shape) {
        (None, _) | (_, None) => Value::Null,
        (Some(_), Some(OutputShape::Object)) => list(vec![Value::Str("o".into())]),
        (Some(Shaped::Keyed(keyed)), _) => {
            let rows = c.db.with_last_result(|rows| rows.to_vec());
            let pairs = keyed
                .into_iter()
                .map(|(key, row)| {
                    let index = rows.iter().position(|r| *r == row).unwrap_or(0);
                    list(vec![bytes_value(key), Value::Int(index as i64)])
                })
                .collect();
            list(vec![Value::Str("k".into()), list(pairs)])
        }
        (Some(Shaped::List(rows)), Some(s)) => list(vec![
            Value::Str("a".into()),
            list(rows.iter().map(|r| row_value(r, s)).collect()),
        ]),
    })
}

/// `[generation, rows]`, rows `null` when `known` is still current.
fn n_last_result(args: &[Value]) -> R {
    let c = ctx(args)?;
    let known = int(&a(args, 1));
    let generation = c.db.result_generation() as i64;
    if generation == known {
        return Ok(list(vec![Value::Int(generation), Value::Null]));
    }
    let rows =
        c.db.with_last_result(|rows| rows.iter().map(row_assoc).collect());
    Ok(list(vec![Value::Int(generation), list(rows)]))
}

/// `['ok', value]`, or `['none']` when `wpdb` has no such property.
fn n_prop_get(args: &[Value]) -> R {
    let c = ctx(args)?;
    Ok(match c.db.property(&string(&a(args, 1))) {
        Some(v) => list(vec![Value::Str("ok".into()), v]),
        None => list(vec![Value::Str("none".into())]),
    })
}

fn n_prop_set(args: &[Value]) -> R {
    let c = ctx(args)?;
    Ok(Value::Bool(
        c.db.set_property(&string(&a(args, 1)), &a(args, 2)),
    ))
}

fn n_set_prefix(args: &[Value]) -> R {
    let c = ctx(args)?;
    let set = !matches!(a(args, 2), Value::Bool(false))
        && (matches!(a(args, 2), Value::Null) || truthy(&a(args, 2)));
    Ok(
        match c.cmd(
            |h| &h.set_prefix,
            SetPrefix {
                prefix: string(&a(args, 1)),
                set_table_names: set,
            },
        )? {
            Ok(old) => Value::Str(old),
            Err(_) => wp_error("invalid_db_prefix", "Invalid database prefix"),
        },
    )
}

fn n_set_blog_id(args: &[Value]) -> R {
    let c = ctx(args)?;
    Ok(Value::Int(c.cmd(
        |h| &h.set_blog_id,
        SetBlogId {
            blog_id: int(&a(args, 1)),
            network_id: int(&a(args, 2)),
        },
    )?))
}

fn n_get_blog_prefix(args: &[Value]) -> R {
    let c = ctx(args)?;
    let blog_id = match a(args, 1) {
        Value::Null => None,
        v => Some(int(&v)),
    };
    Ok(Value::Str(
        c.ask(|h| &h.get_blog_prefix, GetBlogPrefix { blog_id })?,
    ))
}

fn n_tables(args: &[Value]) -> R {
    let c = ctx(args)?;
    let prefix = matches!(a(args, 2), Value::Null) || truthy(&a(args, 2));
    let m = Tables {
        scope: string(&a(args, 1)),
        prefix,
        blog_id: int(&a(args, 3)),
    };
    let tables = c.ask(|h| &h.tables, m)?;
    Ok(if prefix {
        Value::Array(
            tables
                .into_iter()
                .map(|(k, v)| (Key::Str(k), Value::Str(v)))
                .collect(),
        )
    } else {
        list(tables.into_iter().map(|(k, _)| Value::Str(k)).collect())
    })
}

fn n_select(args: &[Value]) -> R {
    let c = ctx(args)?;
    let _ = c.cmd(
        |h| &h.select,
        Select {
            db: string(&a(args, 1)),
        },
    )?;
    Ok(Value::Null)
}

fn n_set_charset(args: &[Value]) -> R {
    let c = ctx(args)?;
    let _ = c.cmd(
        |h| &h.set_charset,
        SetCharset {
            charset: opt_str(&a(args, 1)),
            collate: opt_str(&a(args, 2)),
        },
    )?;
    Ok(Value::Null)
}

fn n_set_sql_mode(args: &[Value]) -> R {
    let c = ctx(args)?;
    let modes = match a(args, 1) {
        Value::Array(items) => items.iter().map(|(_, v)| string(v)).collect(),
        _ => Vec::new(),
    };
    let _ = c.cmd(|h| &h.set_sql_mode, SetSqlMode { modes })?;
    Ok(Value::Null)
}

fn n_init_charset(args: &[Value]) -> R {
    ctx(args)?.db.init_charset();
    Ok(Value::Null)
}

fn n_determine_charset(args: &[Value]) -> R {
    let c = ctx(args)?;
    let (charset, collate) = (string(&a(args, 1)), string(&a(args, 2)));
    let (charset, collate) = if c.db.read_dbh_present() && c.db.db_version().is_ok() {
        CharsetRules::determine_charset(
            &charset,
            &collate,
            &ServerCaps::new(&c.db.db_server_info().unwrap_or_default()),
        )
    } else {
        (charset, collate)
    };
    Ok(Value::Array(vec![
        (Key::Str("charset".into()), Value::Str(charset)),
        (Key::Str("collate".into()), Value::Str(collate)),
    ]))
}

fn n_real_escape(args: &[Value]) -> R {
    let c = ctx(args)?;
    let v = a(args, 1);
    if matches!(v, Value::Array(_) | Value::Opaque(_) | Value::Null) {
        return Ok(Value::Str(String::new()));
    }
    c.ask(|h| &h.escape, Escape { data: v })
}

fn n_escape_data(args: &[Value]) -> R {
    let c = ctx(args)?;
    c.ask(|h| &h.escape, Escape { data: a(args, 1) })
}

fn weak(v: &Value) -> Value {
    match v {
        Value::Array(items) => {
            Value::Array(items.iter().map(|(k, v)| (k.clone(), weak(v))).collect())
        }
        other => bytes_value(weak_escape(&bytes(other).unwrap_or_default())),
    }
}

fn n_weak_escape(args: &[Value]) -> R {
    let c = ctx(args)?;
    if truthy(&a(args, 2)) {
        c.db.host().deprecated_function(
            &string(&a(args, 3)),
            "3.6.0",
            "wpdb::prepare() or esc_sql()",
        );
    }
    Ok(weak(&a(args, 1)))
}

fn n_esc_like(args: &[Value]) -> R {
    let c = ctx(args)?;
    Ok(bytes_value(c.ask(
        |h| &h.esc_like,
        EscLike {
            text: bytes(&a(args, 1)).unwrap_or_default(),
        },
    )?))
}

fn n_quote_identifier(args: &[Value]) -> R {
    Ok(Value::Str(quote_identifier(&string(&a(args, 1)))))
}

fn n_placeholder_escape(args: &[Value]) -> R {
    Ok(Value::Str(
        ctx(args)?.db.placeholder_escape().placeholder().to_string(),
    ))
}

fn n_add_placeholder_escape(args: &[Value]) -> R {
    let c = ctx(args)?;
    Ok(bytes_value(
        c.db.placeholder_escape()
            .add_bytes(&bytes(&a(args, 1)).unwrap_or_default()),
    ))
}

fn n_remove_placeholder_escape(args: &[Value]) -> R {
    let c = ctx(args)?;
    Ok(bytes_value(
        c.db.placeholder_escape()
            .remove_bytes(&bytes(&a(args, 1)).unwrap_or_default()),
    ))
}

fn n_print_error(args: &[Value]) -> R {
    let c = ctx(args)?;
    let message = string(&a(args, 1));
    c.db.print_error(Some(&message));
    Ok(
        if c.db.read_bool("suppress_errors") || !c.db.read_bool("show_errors") {
            Value::Bool(false)
        } else {
            Value::Null
        },
    )
}

fn n_show_errors(args: &[Value]) -> R {
    let c = ctx(args)?;
    let show = matches!(a(args, 1), Value::Null) || truthy(&a(args, 1));
    Ok(Value::Bool(c.db.show_errors(show)))
}

fn n_hide_errors(args: &[Value]) -> R {
    Ok(Value::Bool(ctx(args)?.db.hide_errors()))
}

fn n_suppress_errors(args: &[Value]) -> R {
    let c = ctx(args)?;
    let suppress = matches!(a(args, 1), Value::Null) || truthy(&a(args, 1));
    Ok(Value::Bool(c.db.suppress_errors(suppress)))
}

fn n_flush(args: &[Value]) -> R {
    ctx(args)?.cmd(|h| &h.flush, Flush {})?;
    Ok(Value::Null)
}

fn n_db_connect(args: &[Value]) -> R {
    let c = ctx(args)?;
    let allow = matches!(a(args, 1), Value::Null) || truthy(&a(args, 1));
    Ok(Value::Bool(c.db.db_connect(allow).unwrap_or(false)))
}

fn n_parse_db_host(args: &[Value]) -> R {
    Ok(match DbHost::parse(&string(&a(args, 1))) {
        None => Value::Bool(false),
        Some(h) => list(vec![
            Value::Str(h.host),
            h.port.map_or(Value::Null, |p| Value::Int(i64::from(p))),
            h.socket.map_or(Value::Null, Value::Str),
            Value::Bool(h.is_ipv6),
        ]),
    })
}

fn n_check_connection(args: &[Value]) -> R {
    let c = ctx(args)?;
    let allow = matches!(a(args, 1), Value::Null) || truthy(&a(args, 1));
    Ok(Value::Bool(
        c.cmd(
            |h| &h.check_connection,
            CheckConnection { allow_bail: allow },
        )?
        .unwrap_or(false),
    ))
}

fn n_log_query(args: &[Value]) -> R {
    let c = ctx(args)?;
    let f = |v: &Value| match v {
        Value::Float(f) => *f,
        other => int(other) as f64,
    };
    c.db.log_query(
        &string(&a(args, 1)),
        f(&a(args, 2)),
        &string(&a(args, 3)),
        f(&a(args, 4)),
        a(args, 5),
    );
    Ok(Value::Null)
}

fn n_process_fields(args: &[Value]) -> R {
    let c = ctx(args)?;
    let Some(data) = data(&a(args, 2)) else {
        return Ok(Value::Bool(false));
    };
    Ok(c.db
        .process_fields(&string(&a(args, 1)), &data, formats(&a(args, 3)).as_deref())
        .map_or(Value::Bool(false), |f| fields_to_php(&f)))
}

fn n_process_field_formats(args: &[Value]) -> R {
    let c = ctx(args)?;
    let Some(data) = data(&a(args, 1)) else {
        return Ok(Value::Array(Vec::new()));
    };
    let types = c.db.field_types();
    Ok(fields_to_php(
        &FieldFormats {
            field_types: &types,
        }
        .process_field_formats(&data, formats(&a(args, 2)).as_deref()),
    ))
}

fn n_process_field_charsets(args: &[Value]) -> R {
    let c = ctx(args)?;
    let mut fields = fields_from_php(&a(args, 1));
    let table = string(&a(args, 2));
    for f in fields.iter_mut() {
        f.charset = if f.format == "%d" || f.format == "%f" {
            None
        } else {
            match c.db.get_col_charset(&table, &f.name) {
                Ok(cs) => cs,
                Err(_) => return Ok(Value::Bool(false)),
            }
        };
    }
    Ok(fields_to_php(&fields))
}

fn n_process_field_lengths(args: &[Value]) -> R {
    let c = ctx(args)?;
    let mut fields = fields_from_php(&a(args, 1));
    let table = string(&a(args, 2));
    for f in fields.iter_mut() {
        f.length = if f.format == "%d" || f.format == "%f" {
            None
        } else {
            match c.db.get_col_length(&table, &f.name) {
                Ok(l) => l,
                Err(_) => return Ok(Value::Bool(false)),
            }
        };
    }
    Ok(fields_to_php(&fields))
}

fn n_get_table_charset(args: &[Value]) -> R {
    let c = ctx(args)?;
    Ok(charset_value(c.ask(
        |h| &h.get_table_charset,
        GetTableCharset {
            table: string(&a(args, 1)),
        },
    )?))
}

fn n_get_col_charset(args: &[Value]) -> R {
    let c = ctx(args)?;
    Ok(charset_value(c.ask(
        |h| &h.get_col_charset,
        GetColCharset {
            table: string(&a(args, 1)),
            column: string(&a(args, 2)),
        },
    )?))
}

fn n_get_col_length(args: &[Value]) -> R {
    let c = ctx(args)?;
    Ok(
        match c.ask(
            |h| &h.get_col_length,
            GetColLength {
                table: string(&a(args, 1)),
                column: string(&a(args, 2)),
            },
        )? {
            Ok(l) => col_length(l),
            Err(e) => wp_error("wpdb_get_table_charset_failure", &e.last_error()),
        },
    )
}

fn n_check_ascii(args: &[Value]) -> R {
    Ok(Value::Bool(CharsetRules::check_ascii(
        &bytes(&a(args, 1)).unwrap_or_default(),
    )))
}

fn n_check_safe_collation(args: &[Value]) -> R {
    let c = ctx(args)?;
    Ok(Value::Bool(c.db.check_safe_collation(
        &bytes(&a(args, 1)).unwrap_or_default(),
    )))
}

fn n_strip_invalid_text(args: &[Value]) -> R {
    let c = ctx(args)?;
    let mut fields = fields_from_php(&a(args, 1));
    match c.db.strip_fields(&mut fields) {
        Ok(()) => Ok(fields_to_php(&fields)),
        Err(e) => Ok(wp_error("wpdb_strip_invalid_text_failure", &e.last_error())),
    }
}

fn n_strip_invalid_text_from_query(args: &[Value]) -> R {
    let c = ctx(args)?;
    match c
        .db
        .strip_query_text(&bytes(&a(args, 1)).unwrap_or_default())
    {
        Ok(q) => Ok(bytes_value(q)),
        Err(e) => Ok(wp_error("wpdb_get_table_charset_failure", &e.last_error())),
    }
}

fn n_strip_invalid_text_for_column(args: &[Value]) -> R {
    let c = ctx(args)?;
    let value = a(args, 3);
    if !matches!(value, Value::Str(_) | Value::Bytes(_)) {
        return Ok(value);
    }
    match c.db.strip_invalid_text_for_column(
        &string(&a(args, 1)),
        &string(&a(args, 2)),
        &bytes(&value).unwrap_or_default(),
    ) {
        Ok(v) => Ok(bytes_value(v)),
        Err(e) => Ok(wp_error("wpdb_strip_invalid_text_failure", &e.last_error())),
    }
}

fn n_get_table_from_query(args: &[Value]) -> R {
    Ok(
        get_table_from_query(&bytes(&a(args, 1)).unwrap_or_default())
            .map_or(Value::Bool(false), bytes_value),
    )
}

fn n_get_col_info(args: &[Value]) -> R {
    let c = ctx(args)?;
    let info_type = match a(args, 1) {
        Value::Null => "name".to_string(),
        v => string(&v),
    };
    let col_offset = match a(args, 2) {
        Value::Null => -1,
        v => int(&v),
    };
    c.ask(
        |h| &h.get_col_info,
        GetColInfo {
            info_type,
            col_offset,
        },
    )
}

fn n_timer_start(args: &[Value]) -> R {
    Ok(Value::Bool(ctx(args)?.db.timer_start()))
}

fn n_timer_stop(args: &[Value]) -> R {
    Ok(Value::Float(ctx(args)?.db.timer_stop()))
}

fn n_bail(args: &[Value]) -> R {
    let c = ctx(args)?;
    let code = match a(args, 2) {
        Value::Null => "500".to_string(),
        v => string(&v),
    };
    Ok(Value::Bool(c.db.bail(&string(&a(args, 1)), &code)))
}

fn n_close(args: &[Value]) -> R {
    Ok(Value::Bool(ctx(args)?.cmd(|h| &h.close, Close {})?))
}

fn n_check_database_version(args: &[Value]) -> R {
    let c = ctx(args)?;
    Ok(
        match c
            .db
            .check_database_version(&string(&a(args, 1)), &string(&a(args, 2)))
        {
            Ok(()) => Value::Null,
            Err(WpDbError::Bailed { code, message }) => wp_error(&code, &message),
            Err(e) => wp_error("database_version", &e.last_error()),
        },
    )
}

fn n_supports_collation(args: &[Value]) -> R {
    Ok(Value::Bool(
        ctx(args)?.ask(|h| &h.supports_collation, SupportsCollation {})?,
    ))
}

fn n_get_charset_collate(args: &[Value]) -> R {
    Ok(Value::Str(
        ctx(args)?.ask(|h| &h.get_charset_collate, GetCharsetCollate {})?,
    ))
}

fn n_has_cap(args: &[Value]) -> R {
    let c = ctx(args)?;
    Ok(Value::Bool(c.ask(
        |h| &h.has_cap,
        HasCap {
            cap: string(&a(args, 1)),
        },
    )?))
}

fn n_get_caller(args: &[Value]) -> R {
    Ok(Value::Str(ctx(args)?.db.host().caller()))
}

fn n_db_version(args: &[Value]) -> R {
    Ok(ctx(args)?
        .ask(|h| &h.db_version, DbVersion {})?
        .map_or(Value::Null, Value::Str))
}

fn n_db_server_info(args: &[Value]) -> R {
    Ok(ctx(args)?
        .ask(|h| &h.db_server_info, DbServerInfo {})?
        .map_or(Value::Bool(false), Value::Str))
}

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    type N = fn(&[Value]) -> R;
    let entries: Vec<(&str, N)> = vec![
        ("open", n_open),
        ("construct", n_construct),
        ("query", n_query),
        ("prepare", n_prepare),
        ("insert", n_insert),
        ("replace", n_replace),
        ("insert_replace_helper", n_insert_replace_helper),
        ("update", n_update),
        ("delete", n_delete),
        ("get_var", n_get_var),
        ("get_row", n_get_row),
        ("get_col", n_get_col),
        ("get_results", n_get_results),
        ("last_result", n_last_result),
        ("prop_get", n_prop_get),
        ("prop_set", n_prop_set),
        ("set_prefix", n_set_prefix),
        ("set_blog_id", n_set_blog_id),
        ("get_blog_prefix", n_get_blog_prefix),
        ("tables", n_tables),
        ("select", n_select),
        ("set_charset", n_set_charset),
        ("set_sql_mode", n_set_sql_mode),
        ("init_charset", n_init_charset),
        ("determine_charset", n_determine_charset),
        ("real_escape", n_real_escape),
        ("escape_data", n_escape_data),
        ("weak_escape", n_weak_escape),
        ("esc_like", n_esc_like),
        ("quote_identifier", n_quote_identifier),
        ("placeholder_escape", n_placeholder_escape),
        ("add_placeholder_escape", n_add_placeholder_escape),
        ("remove_placeholder_escape", n_remove_placeholder_escape),
        ("print_error", n_print_error),
        ("show_errors", n_show_errors),
        ("hide_errors", n_hide_errors),
        ("suppress_errors", n_suppress_errors),
        ("flush", n_flush),
        ("db_connect", n_db_connect),
        ("parse_db_host", n_parse_db_host),
        ("check_connection", n_check_connection),
        ("log_query", n_log_query),
        ("process_fields", n_process_fields),
        ("process_field_formats", n_process_field_formats),
        ("process_field_charsets", n_process_field_charsets),
        ("process_field_lengths", n_process_field_lengths),
        ("get_table_charset", n_get_table_charset),
        ("get_col_charset", n_get_col_charset),
        ("get_col_length", n_get_col_length),
        ("check_ascii", n_check_ascii),
        ("check_safe_collation", n_check_safe_collation),
        ("strip_invalid_text", n_strip_invalid_text),
        (
            "strip_invalid_text_from_query",
            n_strip_invalid_text_from_query,
        ),
        (
            "strip_invalid_text_for_column",
            n_strip_invalid_text_for_column,
        ),
        ("get_table_from_query", n_get_table_from_query),
        ("get_col_info", n_get_col_info),
        ("timer_start", n_timer_start),
        ("timer_stop", n_timer_stop),
        ("bail", n_bail),
        ("close", n_close),
        ("check_database_version", n_check_database_version),
        ("supports_collation", n_supports_collation),
        ("get_charset_collate", n_get_charset_collate),
        ("has_cap", n_has_cap),
        ("get_caller", n_get_caller),
        ("db_version", n_db_version),
        ("db_server_info", n_db_server_info),
    ];
    entries
        .into_iter()
        .map(|(name, f)| native::function(&format!("{NATIVE}{name}"), f))
        .collect()
}
