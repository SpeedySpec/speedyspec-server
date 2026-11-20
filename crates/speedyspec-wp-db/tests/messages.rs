mod support;

use std::collections::HashSet;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandBus, CommandHandler, HandlerRef, Query as CqrsQuery, QueryBus, QueryHandler,
};
use speedyspec_wp_db::*;
use support::{Answer, rig, rows};

macro_rules! commands {
    ($c:expr, $keys:expr, $($t:ty),*) => {$(
        assert!(HandlerRef::<$t>::new($c.clone()).handler().is_ok(), stringify!($t));
        $keys.push(<$t as Command>::KEY);
    )*};
}
macro_rules! queries {
    ($c:expr, $keys:expr, $($t:ty),*) => {$(
        assert!($c.make_port::<dyn QueryHandler<$t>>(<$t as CqrsQuery>::KEY).is_ok(), stringify!($t));
        $keys.push(<$t as CqrsQuery>::KEY);
    )*};
}

fn bound() -> (Rc<Container>, Rc<WpDb>, Rc<support::FakeStore>) {
    let r = rig();
    let container = Rc::new(Container::new());
    let store = r.store.clone();
    let db = Rc::new(r.db);
    register_wpdb_handlers(&container, db.clone());
    (container, db, store)
}

#[test]
fn every_key_is_bound() {
    let (c, _, _) = bound();
    let mut keys: Vec<&str> = Vec::new();
    commands!(
        c,
        keys,
        Query,
        Insert,
        Replace,
        Update,
        Delete,
        SetPrefix,
        SetBlogId,
        SetCharset,
        SetSqlMode,
        Select,
        Flush,
        Close,
        CheckConnection
    );
    queries!(
        c,
        keys,
        PrepareSql,
        EscLike,
        Escape,
        GetVar,
        GetRow,
        GetCol,
        GetResults,
        GetColInfo,
        GetTableCharset,
        GetColCharset,
        GetColLength,
        GetBlogPrefix,
        Tables,
        HasCap,
        DbVersion,
        DbServerInfo,
        SupportsCollation,
        GetCharsetCollate
    );
    assert_eq!(keys.len(), 31);
    assert_eq!(keys.iter().collect::<HashSet<_>>().len(), 31);
    assert!(
        keys.iter()
            .all(|k| k.starts_with("SpeedySpec\\WpDb\\") && k.ends_with("Handler"))
    );
}

#[test]
fn dispatch_reaches_wpdb() {
    let (c, _, store) = bound();
    store.answer(Answer::Rows(rows(&["1"], &[&[Some("1")]])));
    let commands = CommandBus::new(c.clone());
    let queries = QueryBus::new(c.clone());
    match commands
        .dispatch(Query {
            sql: b"SELECT 1".to_vec(),
        })
        .unwrap()
        .unwrap()
    {
        QueryOutcome::Rows(rows) => assert_eq!(rows.len(), 1),
        other => panic!("{other:?}"),
    }
    commands
        .dispatch(SetPrefix {
            prefix: "wp_".into(),
            set_table_names: true,
        })
        .unwrap()
        .unwrap();
    assert_eq!(queries.ask(GetBlogPrefix { blog_id: None }).unwrap(), "wp_");
    assert_eq!(
        queries.ask(GetVar { x: 0, y: 0 }).unwrap(),
        Some(b"1".to_vec())
    );
}

struct Override;
impl CommandHandler<Query> for Override {
    fn handle(&self, _: Query) -> Result<QueryOutcome, WpDbError> {
        Ok(QueryOutcome::Done)
    }
}

#[test]
fn php_can_override() {
    let (c, _, _) = bound();
    speedyspec_cqrs::bind_command_handler::<Query>(&c, |_, _| Ok(Rc::new(Override)));
    assert_eq!(
        CommandBus::new(c)
            .dispatch(Query {
                sql: b"SELECT 1".to_vec()
            })
            .unwrap()
            .unwrap(),
        QueryOutcome::Done
    );
}
