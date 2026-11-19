//! The behaviours of Laravel's container tests, against this container, plus the two lifetimes and the override flag.

use std::cell::Cell;
use std::rc::Rc;

use speedyspec_container::{Container, ContainerError, Lifetime, Params, Resolved};

fn text(value: &Resolved) -> String {
    value
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_else(|| "<not a string>".to_string())
}

fn string(s: &str) -> Resolved {
    Rc::new(s.to_string())
}

/// A factory that counts how many times it built.
fn counting(
    counter: Rc<Cell<u32>>,
    label: &'static str,
) -> impl Fn(&Container, &Params) -> Result<Resolved, ContainerError> {
    move |_, _| {
        counter.set(counter.get() + 1);
        Ok(string(&format!("{label}{}", counter.get())))
    }
}

#[test]
fn singleton_is_shared() {
    let c = Container::new();
    let built = Rc::new(Cell::new(0));
    c.singleton("db", counting(built.clone(), "db"));
    assert_eq!(text(&c.make("db").unwrap()), "db1");
    assert_eq!(text(&c.make("db").unwrap()), "db1");
    assert_eq!(built.get(), 1);
}

#[test]
fn bind_is_not_shared() {
    let c = Container::new();
    let built = Rc::new(Cell::new(0));
    c.bind("req", counting(built.clone(), "r"));
    assert_eq!(text(&c.make("req").unwrap()), "r1");
    assert_eq!(text(&c.make("req").unwrap()), "r2");
}

#[test]
fn scoped_is_shared_until_end_request() {
    let c = Container::new();
    let built = Rc::new(Cell::new(0));
    c.scoped("ctx", counting(built.clone(), "c"));
    assert_eq!(text(&c.make("ctx").unwrap()), "c1");
    assert_eq!(text(&c.make("ctx").unwrap()), "c1");
    c.end_request();
    assert_eq!(
        text(&c.make("ctx").unwrap()),
        "c2",
        "a new request gets a new scoped instance"
    );
    assert!(
        c.has("ctx"),
        "the scoped binding itself is a worker-lifetime binding and stays"
    );
}

#[test]
fn instance_is_returned_as_is() {
    let c = Container::new();
    let value = string("exact");
    c.instance("thing", value.clone());
    assert!(Rc::ptr_eq(&c.make("thing").unwrap(), &value));
}

#[test]
fn bind_if_keeps_the_first() {
    let c = Container::new();
    c.bind("k", |_, _| Ok(string("first")));
    c.bind_if("k", |_, _| Ok(string("second")));
    c.singleton_if("k", |_, _| Ok(string("third")));
    c.scoped_if("k", |_, _| Ok(string("fourth")));
    assert_eq!(text(&c.make("k").unwrap()), "first");
}

#[test]
fn alias_resolves_its_target() {
    let c = Container::new();
    c.singleton("Foo\\Real", |_, _| Ok(string("real")));
    c.alias("Foo\\Real", "foo");
    c.alias("foo", "f");
    assert_eq!(text(&c.make("f").unwrap()), "real");
    assert!(c.is_alias("f"));
    assert_eq!(c.get_alias("f"), "Foo\\Real");
    assert!(c.has("f"));
}

#[test]
fn circular_alias_is_refused() {
    let c = Container::new();
    c.alias("a", "b");
    let err = c.try_alias("b", "a").unwrap_err();
    assert!(err.to_string().contains("aliased to itself"), "{err}");
}

#[test]
fn extend_decorates_and_applies_to_existing_singleton() {
    let c = Container::new();
    c.singleton("greeting", |_, _| Ok(string("hello")));
    let before = c.make("greeting").unwrap();
    c.extend("greeting", |value, _| {
        Ok(string(&format!("{}!", text(&value))))
    });
    assert_eq!(text(&c.make("greeting").unwrap()), "hello!");
    assert!(
        !Rc::ptr_eq(&before, &c.make("greeting").unwrap()),
        "the shared instance was replaced"
    );
    c.bind("fresh", |_, _| Ok(string("a")));
    c.extend("fresh", |v, _| Ok(string(&format!("{}b", text(&v)))));
    c.extend("fresh", |v, _| Ok(string(&format!("{}c", text(&v)))));
    assert_eq!(
        text(&c.make("fresh").unwrap()),
        "abc",
        "extenders run in order"
    );
}

#[test]
fn contextual_binding_gives_per_consumer() {
    let c = Container::new();
    c.bind("Store", |_, _| Ok(string("default store")));
    c.bind("Reports", |c, _| {
        Ok(string(&format!("reports use {}", text(&c.make("Store")?))))
    });
    c.bind("Uploads", |c, _| {
        Ok(string(&format!("uploads use {}", text(&c.make("Store")?))))
    });
    c.when("Uploads")
        .needs("Store")
        .give(|_, _| Ok(string("file store")));
    c.when("Reports").needs("Store").give_key("Other");
    c.bind("Other", |_, _| Ok(string("other store")));
    assert_eq!(text(&c.make("Uploads").unwrap()), "uploads use file store");
    assert_eq!(text(&c.make("Reports").unwrap()), "reports use other store");
    assert_eq!(text(&c.make("Store").unwrap()), "default store");
}

#[test]
fn tagged_returns_in_tag_order() {
    let c = Container::new();
    c.bind("a", |_, _| Ok(string("A")));
    c.bind("b", |_, _| Ok(string("B")));
    c.bind("z", |_, _| Ok(string("Z")));
    c.tag(&["b", "a"], "letters");
    c.tag(&["z"], "letters");
    let all: Vec<String> = c.tagged("letters").unwrap().iter().map(text).collect();
    assert_eq!(all, vec!["B", "A", "Z"]);
    assert!(c.tagged("none").unwrap().is_empty());
}

#[test]
fn resolving_callbacks_run_in_order_before_after() {
    let c = Container::new();
    let log = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    c.bind("svc", |_, _| Ok(string("svc")));
    let l = log.clone();
    c.before_resolving(Some("svc"), move |key, _| {
        l.borrow_mut().push(format!("before {key}"))
    });
    let l = log.clone();
    c.resolving(Some("svc"), move |v, _| {
        l.borrow_mut().push(format!("resolving {}", text(v)))
    });
    let l = log.clone();
    c.resolving(None, move |_, _| {
        l.borrow_mut().push("resolving any".to_string())
    });
    let l = log.clone();
    c.after_resolving(Some("svc"), move |_, _| {
        l.borrow_mut().push("after".to_string())
    });
    c.make("svc").unwrap();
    assert_eq!(
        *log.borrow(),
        vec!["before svc", "resolving svc", "resolving any", "after"]
    );
}

#[test]
fn circular_dependency_names_the_chain() {
    let c = Container::new();
    c.bind("a", |c, _| c.make("b"));
    c.bind("b", |c, _| c.make("a"));
    let err = c.make("a").unwrap_err();
    assert!(
        matches!(err, ContainerError::Circular(ref chain) if chain == &["a", "b", "a"]),
        "{err:?}"
    );
    assert!(err.to_string().contains("a -> b -> a"), "{err}");
}

#[test]
fn unresolvable_names_the_key() {
    let c = Container::new();
    let err = c.make("Nothing\\Here").unwrap_err();
    assert!(err.to_string().contains("Nothing\\Here"), "{err}");
}

#[test]
fn make_with_builds_fresh_with_params() {
    let c = Container::new();
    c.singleton("greeter", |_, params: &Params| {
        Ok(string(&format!(
            "hi {}",
            params
                .get("name")
                .map(text)
                .unwrap_or_else(|| "nobody".into())
        )))
    });
    let mut params = Params::new();
    params.insert("name", string("ada"));
    assert_eq!(text(&c.make_with("greeter", &params).unwrap()), "hi ada");
    assert_eq!(
        text(&c.make("greeter").unwrap()),
        "hi nobody",
        "make_with never becomes the shared instance"
    );
}

#[test]
fn factory_makes_on_each_call() {
    let c = Container::new();
    let built = Rc::new(Cell::new(0));
    c.bind("n", counting(built.clone(), "n"));
    let make = c.factory("n");
    assert_eq!(text(&make().unwrap()), "n1");
    assert_eq!(text(&make().unwrap()), "n2");
}

#[test]
fn forget_scoped_instances_keeps_singletons() {
    let c = Container::new();
    let (s, p) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    c.singleton("single", counting(s.clone(), "s"));
    c.scoped("scope", counting(p.clone(), "p"));
    c.make("single").unwrap();
    c.make("scope").unwrap();
    c.forget_scoped_instances();
    assert_eq!(text(&c.make("single").unwrap()), "s1");
    assert_eq!(text(&c.make("scope").unwrap()), "p2");
    c.forget_instance("single");
    assert_eq!(text(&c.make("single").unwrap()), "s2");
    c.forget_instances();
    assert_eq!(text(&c.make("single").unwrap()), "s3");
    c.flush();
    assert!(!c.has("single"));
}

#[test]
fn end_request_removes_request_lifetime_everything() {
    let c = Container::new();
    c.singleton("port", |_, _| Ok(string("rust")));
    c.tag(&["port"], "t");
    {
        let _request = c.registering(Lifetime::Request);
        c.bind("port", |_, _| Ok(string("php override")));
        c.instance("only-this-request", string("x"));
        c.alias("port", "p");
        c.extend("port", |v, _| Ok(string(&format!("{} extended", text(&v)))));
        c.tag(&["only-this-request"], "t");
        c.when("x")
            .needs("port")
            .give(|_, _| Ok(string("contextual")));
    }
    assert_eq!(text(&c.make("p").unwrap()), "php override extended");
    assert_eq!(c.tagged("t").unwrap().len(), 2);
    c.end_request();
    assert_eq!(
        text(&c.make("port").unwrap()),
        "rust",
        "the worker-lifetime binding is back"
    );
    assert!(!c.has("only-this-request"));
    assert!(!c.is_alias("p"));
    assert_eq!(c.tagged("t").unwrap().len(), 1);
}

#[test]
fn is_foreign_tracks_the_current_binding() {
    let c = Container::new();
    c.singleton("port", |_, _| Ok(string("rust")));
    assert!(!c.is_foreign("port"));
    {
        let _request = c.registering(Lifetime::Request);
        c.bind_foreign("port", |_, _| Ok(string("php")));
    }
    assert!(c.is_foreign("port"));
    c.alias("port", "alias-of-port");
    assert!(c.is_foreign("alias-of-port"));
    c.end_request();
    assert!(!c.is_foreign("port"), "the override ends with the request");
}

#[test]
fn typed_ports_resolve_as_trait_objects() {
    trait Greeter {
        fn greet(&self) -> String;
    }
    struct English;
    impl Greeter for English {
        fn greet(&self) -> String {
            "hello".into()
        }
    }
    let c = Container::new();
    c.bind_port::<dyn Greeter>("Greeter", |_, _| Ok(Rc::new(English) as Rc<dyn Greeter>));
    let greeter: Rc<dyn Greeter> = c.make_port::<dyn Greeter>("Greeter").unwrap();
    assert_eq!(greeter.greet(), "hello");
    let err = match c.make_port::<String>("Greeter") {
        Ok(_) => panic!("a wrong type is refused"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("Greeter"), "{err}");
}
