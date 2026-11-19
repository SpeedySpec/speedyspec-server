//! The buses: handlers from the container, middleware order, direct dispatch and its cache.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandBus, CommandHandler, HandlerRef, LoggingMiddleware, Message, Middleware, Next,
    Outcome, Query, QueryBus, QueryHandler, bind_command_handler, bind_query_handler,
};

struct AddPost {
    title: String,
}

impl Command for AddPost {
    type Result = String;
    const KEY: &'static str = "SpeedySpec\\Posts\\AddPostHandler";
}

struct CountPosts;

impl Query for CountPosts {
    type Result = i64;
    const KEY: &'static str = "SpeedySpec\\Posts\\CountPostsHandler";
}

struct Adds(&'static str);

impl CommandHandler<AddPost> for Adds {
    fn handle(&self, command: AddPost) -> String {
        format!("{}: {}", self.0, command.title)
    }
}

struct Counts;

impl QueryHandler<CountPosts> for Counts {
    fn handle(&self, _query: CountPosts) -> i64 {
        3
    }
}

/// Records each message it sees, before and after the rest of the chain.
struct Trace(&'static str, Rc<RefCell<Vec<String>>>);

impl Middleware for Trace {
    fn around(&self, message: &dyn Message, next: Next<'_>) -> Outcome {
        self.1
            .borrow_mut()
            .push(format!("{} before {}", self.0, message.key()));
        let outcome = next.run();
        self.1.borrow_mut().push(format!("{} after", self.0));
        outcome
    }
}

fn container() -> Rc<Container> {
    let c = Rc::new(Container::new());
    bind_command_handler::<AddPost>(&c, |_, _| Ok(Rc::new(Adds("rust"))));
    bind_query_handler::<CountPosts>(&c, |_, _| Ok(Rc::new(Counts)));
    c
}

#[test]
fn middleware_runs_in_order() {
    let c = container();
    let trace = Rc::new(RefCell::new(Vec::new()));
    let bus = CommandBus::new(c.clone());
    bus.add_middleware(Rc::new(Trace("first", trace.clone())));
    bus.add_middleware(Rc::new(Trace("second", trace.clone())));
    let result = bus
        .dispatch(AddPost {
            title: "hello".into(),
        })
        .unwrap();
    assert_eq!(result, "rust: hello");
    assert_eq!(
        *trace.borrow(),
        vec![
            "first before SpeedySpec\\Posts\\AddPostHandler",
            "second before SpeedySpec\\Posts\\AddPostHandler",
            "second after",
            "first after",
        ]
    );

    let queries = QueryBus::new(c);
    let logged = Rc::new(RefCell::new(Vec::new()));
    let sink = logged.clone();
    queries.add_middleware(Rc::new(LoggingMiddleware::new(move |line: &str| {
        sink.borrow_mut().push(line.to_string())
    })));
    assert_eq!(queries.ask(CountPosts).unwrap(), 3);
    assert_eq!(logged.borrow().len(), 2, "{:?}", logged.borrow());
    assert!(logged.borrow()[0].contains("CountPostsHandler"));
}

#[test]
fn direct_dispatch_skips_middleware() {
    let c = container();
    let trace = Rc::new(RefCell::new(Vec::new()));
    let bus = CommandBus::new(c.clone());
    bus.add_middleware(Rc::new(Trace("only", trace.clone())));
    let direct = HandlerRef::<AddPost>::new(c);
    assert_eq!(
        direct.call(AddPost { title: "x".into() }).unwrap(),
        "rust: x"
    );
    assert!(trace.borrow().is_empty());
}

#[test]
fn handler_ref_follows_rebinding() {
    let c = container();
    let direct = HandlerRef::<AddPost>::new(c.clone());
    assert_eq!(
        direct.call(AddPost { title: "a".into() }).unwrap(),
        "rust: a"
    );
    bind_command_handler::<AddPost>(&c, |_, _| Ok(Rc::new(Adds("rebound"))));
    assert_eq!(
        direct.call(AddPost { title: "b".into() }).unwrap(),
        "rebound: b"
    );
}

#[test]
fn handler_ref_resolves_once() {
    let c = Rc::new(Container::new());
    let built = Rc::new(RefCell::new(0));
    let count = built.clone();
    bind_command_handler::<AddPost>(&c, move |_, _| {
        *count.borrow_mut() += 1;
        Ok(Rc::new(Adds("rust")))
    });
    let direct = HandlerRef::<AddPost>::new(c);
    direct.call(AddPost { title: "a".into() }).unwrap();
    direct.call(AddPost { title: "b".into() }).unwrap();
    assert_eq!(*built.borrow(), 1);
}

#[test]
fn missing_handler_is_an_error_naming_the_key() {
    let bus = CommandBus::new(Rc::new(Container::new()));
    let err = bus.dispatch(AddPost { title: "x".into() }).unwrap_err();
    assert!(err.to_string().contains("AddPostHandler"), "{err}");
}

#[test]
fn query_bus_refuses_command() {
    trybuild::TestCases::new().compile_fail("tests/ui/query_bus_refuses_command.rs");
}
