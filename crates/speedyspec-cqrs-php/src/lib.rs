//! The buses from PHP.
//!
//! [`CqrsArea`] adds `SpeedySpec\Cqrs\CommandBus::dispatch($key, $command)` and `QueryBus::ask($key, $query)` to PHP
//! (a facade declared at every request start, over native functions), for the messages registered with
//! [`CqrsArea::command`] and [`CqrsArea::query`]; both go through the Rust buses and their middleware.
//!
//! [`php_command`] and [`php_query`] let PHP override a handler: a PHP class (or closure result) bound to the
//! message's key, with a `handle($message)` method, becomes the handler for Rust callers too, for that request.

use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;

use speedyspec_container_php::ContainerArea;
use speedyspec_cqrs::{Command, CommandBus, CommandHandler, Query, QueryBus, QueryHandler};
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{FromPhp, PhpObject, ToPhp, Value};

const FACADE: &str = include_str!("buses.php");
const EXCEPTION: &str = "SpeedySpec\\Cqrs\\BusException";

/// A PHP object handling `C` (its `handle` method).
pub struct PhpCommandHandler<C>(PhpObject, PhantomData<fn(C)>);

impl<C> PhpCommandHandler<C> {
    pub fn new(object: PhpObject) -> Self {
        Self(object, PhantomData)
    }
}

impl<C: Command + ToPhp> CommandHandler<C> for PhpCommandHandler<C>
where
    C::Result: FromPhp,
{
    fn handle(&self, command: C) -> C::Result {
        handle(&self.0, C::KEY, command.to_php())
    }
}

/// A PHP object handling `Q` (its `handle` method).
pub struct PhpQueryHandler<Q>(PhpObject, PhantomData<fn(Q)>);

impl<Q> PhpQueryHandler<Q> {
    pub fn new(object: PhpObject) -> Self {
        Self(object, PhantomData)
    }
}

impl<Q: Query + ToPhp> QueryHandler<Q> for PhpQueryHandler<Q>
where
    Q::Result: FromPhp,
{
    fn handle(&self, query: Q) -> Q::Result {
        handle(&self.0, Q::KEY, query.to_php())
    }
}

fn handle<R: FromPhp>(object: &PhpObject, key: &str, message: Value) -> R {
    match object.call("handle", vec![message]) {
        Ok(value) => R::from_php(value).unwrap_or_else(|why| R::from_failure(key, "handle", why)),
        Err(err) => R::from_failure(key, "handle", err.to_string()),
    }
}

/// Lets PHP override the handler of `C` (see the crate docs).
pub fn php_command<C: Command + ToPhp>(area: ContainerArea) -> ContainerArea
where
    C::Result: FromPhp,
{
    area.port::<dyn CommandHandler<C>>(C::KEY, |object| {
        Rc::new(PhpCommandHandler::<C>::new(object))
    })
}

/// Lets PHP override the handler of `Q` (see the crate docs).
pub fn php_query<Q: Query + ToPhp>(area: ContainerArea) -> ContainerArea
where
    Q::Result: FromPhp,
{
    area.port::<dyn QueryHandler<Q>>(Q::KEY, |object| Rc::new(PhpQueryHandler::<Q>::new(object)))
}

type Route = Rc<dyn Fn(Value) -> Result<Value, NativeError>>;

#[derive(Default)]
struct Routes {
    commands: HashMap<&'static str, Route>,
    queries: HashMap<&'static str, Route>,
}

thread_local! {
    static ROUTES: RefCell<Option<Rc<RefCell<Routes>>>> = const { RefCell::new(None) };
}

/// The native area that gives PHP the buses.
pub struct CqrsArea {
    commands: Rc<CommandBus>,
    queries: Rc<QueryBus>,
    routes: Rc<RefCell<Routes>>,
}

impl CqrsArea {
    pub fn new(commands: Rc<CommandBus>, queries: Rc<QueryBus>) -> CqrsArea {
        let routes = Rc::new(RefCell::new(Routes::default()));
        ROUTES.with(|r| *r.borrow_mut() = Some(routes.clone()));
        CqrsArea {
            commands,
            queries,
            routes,
        }
    }

    /// PHP may dispatch `C`: its payload converts to `C`, its result back to PHP.
    pub fn command<C: Command + FromPhp>(self) -> Self
    where
        C::Result: ToPhp,
    {
        let bus = self.commands.clone();
        let route: Route = Rc::new(move |payload| {
            let command = C::from_php(payload).map_err(|why| bus_error(C::KEY, why))?;
            bus.dispatch(command)
                .map(ToPhp::to_php)
                .map_err(|err| bus_error(C::KEY, err.to_string()))
        });
        self.routes.borrow_mut().commands.insert(C::KEY, route);
        self
    }

    /// PHP may ask `Q`: its payload converts to `Q`, its result back to PHP.
    pub fn query<Q: Query + FromPhp>(self) -> Self
    where
        Q::Result: ToPhp,
    {
        let bus = self.queries.clone();
        let route: Route = Rc::new(move |payload| {
            let query = Q::from_php(payload).map_err(|why| bus_error(Q::KEY, why))?;
            bus.ask(query)
                .map(ToPhp::to_php)
                .map_err(|err| bus_error(Q::KEY, err.to_string()))
        });
        self.routes.borrow_mut().queries.insert(Q::KEY, route);
        self
    }
}

fn bus_error(key: &str, why: String) -> NativeError {
    NativeError::throw(EXCEPTION, format!("{key}: {why}"))
}

fn route(args: &[Value], commands: bool) -> Result<Value, NativeError> {
    let Some(Value::Str(key)) = args.first() else {
        return Err(NativeError::throw(
            EXCEPTION,
            "the handler key must be a string",
        ));
    };
    let routes = ROUTES
        .with(|r| r.borrow().clone())
        .ok_or_else(|| bus_error(key, "the buses are not available".into()))?;
    let route = {
        let routes = routes.borrow();
        let table = if commands {
            &routes.commands
        } else {
            &routes.queries
        };
        table.get(key.as_str()).cloned()
    };
    let kind = if commands { "command" } else { "query" };
    let route = route.ok_or_else(|| {
        bus_error(
            key,
            format!("no {kind} with this handler key is open to PHP"),
        )
    })?;
    route(args.get(1).cloned().unwrap_or(Value::Null))
}

impl NativeArea for CqrsArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        vec![
            native::function("SpeedySpec\\Cqrs\\Native\\dispatch", |args| {
                route(args, true)
            }),
            native::function("SpeedySpec\\Cqrs\\Native\\ask", |args| route(args, false)),
        ]
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://cqrs", FACADE) {
            eprintln!("speedyspec_wordpress: declaring the SpeedySpec\\Cqrs buses failed: {err}");
        }
    }
}
