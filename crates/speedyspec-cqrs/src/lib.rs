//! Command and query buses.
//!
//! Every use case is a [`Command`] (changes state) or a [`Query`] (reads it), and has one handler, bound in the
//! service container under the message's `KEY` as a port (`dyn CommandHandler<C>` or `dyn QueryHandler<Q>`). PHP
//! overrides a handler by binding the same key (see `speedyspec-cqrs-php`).
//!
//! [`CommandBus::dispatch`] and [`QueryBus::ask`] resolve the handler for each message and run the bus's
//! [`Middleware`] around it, in registration order. [`HandlerRef`] and [`QueryRef`] are direct dispatch: the handler
//! is resolved once and called with no middleware, and resolved again only when its binding changes.

use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use speedyspec_container::{Container, ContainerError, Params};

/// A use case that changes state. `KEY` is its handler's key in the container.
pub trait Command: 'static {
    type Result: 'static;
    const KEY: &'static str;
}

/// A use case that reads state. `KEY` is its handler's key in the container.
pub trait Query: 'static {
    type Result: 'static;
    const KEY: &'static str;
}

pub trait CommandHandler<C: Command> {
    fn handle(&self, command: C) -> C::Result;
}

pub trait QueryHandler<Q: Query> {
    fn handle(&self, query: Q) -> Q::Result;
}

/// Binds the handler of `C` (shared for the container's life, or until rebound).
pub fn bind_command_handler<C: Command>(
    container: &Container,
    factory: impl Fn(&Container, &Params) -> Result<Rc<dyn CommandHandler<C>>, ContainerError> + 'static,
) {
    container.singleton_port::<dyn CommandHandler<C>>(C::KEY, factory);
}

/// Binds the handler of `Q` (shared for the container's life, or until rebound).
pub fn bind_query_handler<Q: Query>(
    container: &Container,
    factory: impl Fn(&Container, &Params) -> Result<Rc<dyn QueryHandler<Q>>, ContainerError> + 'static,
) {
    container.singleton_port::<dyn QueryHandler<Q>>(Q::KEY, factory);
}

/// Whether a message is a command or a query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Command,
    Query,
}

/// What middleware sees of a message.
pub trait Message {
    /// The handler's key.
    fn key(&self) -> &'static str;
    fn kind(&self) -> Kind;
    /// The Rust type of the message.
    fn type_name(&self) -> &'static str;
}

struct Envelope {
    key: &'static str,
    kind: Kind,
    type_name: &'static str,
}

impl Message for Envelope {
    fn key(&self) -> &'static str {
        self.key
    }

    fn kind(&self) -> Kind {
        self.kind
    }

    fn type_name(&self) -> &'static str {
        self.type_name
    }
}

/// What the rest of a middleware chain produced: the handler's result, boxed, or why there is none.
pub type Outcome = Result<Box<dyn Any>, BusError>;

/// The rest of the chain, for a middleware to run (or not).
pub struct Next<'a>(&'a dyn Fn() -> Outcome);

impl Next<'_> {
    pub fn run(self) -> Outcome {
        (self.0)()
    }
}

/// Runs around every message a bus handles.
pub trait Middleware {
    fn around(&self, message: &dyn Message, next: Next<'_>) -> Outcome;
}

/// Why a message was not handled.
#[derive(Debug, thiserror::Error)]
pub enum BusError {
    #[error("no handler for {key}: {source}")]
    NoHandler {
        key: &'static str,
        #[source]
        source: ContainerError,
    },
    #[error("a middleware answered {key} with something that is not its result")]
    WrongOutcome { key: &'static str },
    #[error("{key} was not handled: {why}")]
    Refused { key: &'static str, why: String },
}

/// Logs each message before and after it is handled.
pub struct LoggingMiddleware {
    sink: Box<dyn Fn(&str)>,
}

impl LoggingMiddleware {
    pub fn new(sink: impl Fn(&str) + 'static) -> LoggingMiddleware {
        LoggingMiddleware {
            sink: Box::new(sink),
        }
    }
}

impl Middleware for LoggingMiddleware {
    fn around(&self, message: &dyn Message, next: Next<'_>) -> Outcome {
        let kind = match message.kind() {
            Kind::Command => "command",
            Kind::Query => "query",
        };
        (self.sink)(&format!(
            "{kind} {} ({})",
            message.key(),
            message.type_name()
        ));
        let outcome = next.run();
        match &outcome {
            Ok(_) => (self.sink)(&format!("{kind} {} handled", message.key())),
            Err(err) => (self.sink)(&format!("{kind} {} failed: {err}", message.key())),
        }
        outcome
    }
}

#[derive(Default)]
struct Chain(RefCell<Vec<Rc<dyn Middleware>>>);

impl Chain {
    /// Runs the middleware from `index` on, then `handle`.
    fn run(
        middleware: &[Rc<dyn Middleware>],
        message: &dyn Message,
        handle: &dyn Fn() -> Outcome,
    ) -> Outcome {
        match middleware.split_first() {
            None => handle(),
            Some((first, rest)) => {
                let next = || Chain::run(rest, message, handle);
                first.around(message, Next(&next))
            }
        }
    }

    fn handle<R: 'static>(
        &self,
        envelope: Envelope,
        handle: impl FnOnce() -> Result<R, BusError>,
    ) -> Result<R, BusError> {
        let key = envelope.key;
        let middleware = self.0.borrow().clone();
        let handle = RefCell::new(Some(handle));
        let once = || match handle.borrow_mut().take() {
            Some(handle) => handle().map(|result| Box::new(result) as Box<dyn Any>),
            None => Err(BusError::Refused {
                key,
                why: "a middleware ran the handler twice".into(),
            }),
        };
        let outcome = Chain::run(&middleware, &envelope, &once)?;
        outcome
            .downcast::<R>()
            .map(|result| *result)
            .map_err(|_| BusError::WrongOutcome { key })
    }
}

/// Dispatches commands to their handlers, through the middleware.
pub struct CommandBus {
    container: Rc<Container>,
    middleware: Chain,
}

impl CommandBus {
    pub fn new(container: Rc<Container>) -> CommandBus {
        CommandBus {
            container,
            middleware: Chain::default(),
        }
    }

    /// Adds a middleware; it runs inside the ones added before it.
    pub fn add_middleware(&self, middleware: Rc<dyn Middleware>) {
        self.middleware.0.borrow_mut().push(middleware);
    }

    pub fn dispatch<C: Command>(&self, command: C) -> Result<C::Result, BusError> {
        let envelope = Envelope {
            key: C::KEY,
            kind: Kind::Command,
            type_name: std::any::type_name::<C>(),
        };
        self.middleware.handle(envelope, || {
            let handler = command_handler::<C>(&self.container)?;
            Ok(handler.handle(command))
        })
    }
}

/// Answers queries with their handlers, through the middleware.
pub struct QueryBus {
    container: Rc<Container>,
    middleware: Chain,
}

impl QueryBus {
    pub fn new(container: Rc<Container>) -> QueryBus {
        QueryBus {
            container,
            middleware: Chain::default(),
        }
    }

    /// Adds a middleware; it runs inside the ones added before it.
    pub fn add_middleware(&self, middleware: Rc<dyn Middleware>) {
        self.middleware.0.borrow_mut().push(middleware);
    }

    pub fn ask<Q: Query>(&self, query: Q) -> Result<Q::Result, BusError> {
        let envelope = Envelope {
            key: Q::KEY,
            kind: Kind::Query,
            type_name: std::any::type_name::<Q>(),
        };
        self.middleware.handle(envelope, || {
            let handler = query_handler::<Q>(&self.container)?;
            Ok(handler.handle(query))
        })
    }
}

fn command_handler<C: Command>(
    container: &Container,
) -> Result<Rc<dyn CommandHandler<C>>, BusError> {
    container
        .make_port::<dyn CommandHandler<C>>(C::KEY)
        .map_err(|source| BusError::NoHandler {
            key: C::KEY,
            source,
        })
}

fn query_handler<Q: Query>(container: &Container) -> Result<Rc<dyn QueryHandler<Q>>, BusError> {
    container
        .make_port::<dyn QueryHandler<Q>>(Q::KEY)
        .map_err(|source| BusError::NoHandler {
            key: Q::KEY,
            source,
        })
}

/// A cached handler and the binding it came from.
struct Cached<H: ?Sized> {
    target: String,
    generation: u64,
    handler: Rc<H>,
}

/// Resolves through `resolve` unless the cached handler's binding is unchanged.
fn cached<H: ?Sized>(
    container: &Container,
    key: &str,
    cache: &RefCell<Option<Cached<H>>>,
    resolve: impl FnOnce() -> Result<Rc<H>, BusError>,
) -> Result<Rc<H>, BusError> {
    let target = container.get_alias(key);
    let generation = container.generation(key);
    if let Some(hit) = cache.borrow().as_ref()
        && hit.target == target
        && hit.generation == generation
    {
        return Ok(hit.handler.clone());
    }
    let handler = resolve()?;
    *cache.borrow_mut() = Some(Cached {
        target,
        generation,
        handler: handler.clone(),
    });
    Ok(handler)
}

/// Direct dispatch of `C`: its handler, resolved once and called with no middleware.
pub struct HandlerRef<C: Command> {
    container: Rc<Container>,
    cache: RefCell<Option<Cached<dyn CommandHandler<C>>>>,
    _command: PhantomData<fn(C)>,
}

impl<C: Command> HandlerRef<C> {
    pub fn new(container: Rc<Container>) -> HandlerRef<C> {
        HandlerRef {
            container,
            cache: RefCell::new(None),
            _command: PhantomData,
        }
    }

    pub fn call(&self, command: C) -> Result<C::Result, BusError> {
        Ok(self.handler()?.handle(command))
    }

    /// The handler bound now (resolved again only when its binding changed), for a caller that needs to know
    /// which one it is.
    pub fn handler(&self) -> Result<Rc<dyn CommandHandler<C>>, BusError> {
        cached(&self.container, C::KEY, &self.cache, || {
            command_handler::<C>(&self.container)
        })
    }
}

/// Direct dispatch of `Q`, as [`HandlerRef`] is for commands.
pub struct QueryRef<Q: Query> {
    container: Rc<Container>,
    cache: RefCell<Option<Cached<dyn QueryHandler<Q>>>>,
    _query: PhantomData<fn(Q)>,
}

impl<Q: Query> QueryRef<Q> {
    pub fn new(container: Rc<Container>) -> QueryRef<Q> {
        QueryRef {
            container,
            cache: RefCell::new(None),
            _query: PhantomData,
        }
    }

    pub fn call(&self, query: Q) -> Result<Q::Result, BusError> {
        let handler = cached(&self.container, Q::KEY, &self.cache, || {
            query_handler::<Q>(&self.container)
        })?;
        Ok(handler.handle(query))
    }
}
