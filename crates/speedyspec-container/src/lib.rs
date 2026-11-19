//! The service container: Laravel's container's functionality, in Rust.
//!
//! Every key (a port's name, a class or an alias) holds a stack of entries: bindings (a factory, transient,
//! singleton or scoped) and instances. The top entry answers. Each entry has a [`Lifetime`]: `Worker` entries live as
//! long as the process; `Request` entries (everything PHP registers) are removed by [`Container::end_request`], which
//! puts every key back on its worker-lifetime entry. An entry registered by a language bridge is *foreign*
//! ([`Container::is_foreign`]): that is the override flag, and a Rust binding never involves the bridge.
//!
//! A container belongs to one thread (the worker's, alongside PHP), so values are `Rc`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// A resolved value.
pub type Resolved = Rc<dyn Any>;

/// A value with two faces: the port a Rust caller gets from [`Container::make_port`], and the value it came from
/// in another language (a PHP object implementing the port's twin), which that language's bridge hands back.
pub struct Bridged {
    /// An `Rc<P>` for the port `P`.
    pub port: Resolved,
    pub foreign: Resolved,
}

/// What a binding runs to build its value.
pub type Factory = Rc<dyn Fn(&Container, &Params) -> Result<Resolved, ContainerError>>;

type Extender = Rc<dyn Fn(Resolved, &Container) -> Result<Resolved, ContainerError>>;
type KeyCallback = Rc<dyn Fn(&str, &Container)>;
type ValueCallback = Rc<dyn Fn(&Resolved, &Container)>;

/// How long a registration lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifetime {
    /// As long as the process.
    Worker,
    /// Until [`Container::end_request`].
    Request,
}

/// Parameters for [`Container::make_with`].
#[derive(Clone, Default)]
pub struct Params(HashMap<String, Resolved>);

impl Params {
    pub fn new() -> Params {
        Params::default()
    }

    pub fn insert(&mut self, name: &str, value: Resolved) {
        self.0.insert(name.to_string(), value);
    }

    pub fn get(&self, name: &str) -> Option<&Resolved> {
        self.0.get(name)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Resolved)> {
        self.0.iter().map(|(name, value)| (name.as_str(), value))
    }
}

/// Why a key could not be resolved. Every message names the key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContainerError {
    #[error("{0} cannot be resolved: {1}")]
    Unresolvable(String, String),
    #[error("circular dependency: {}", .0.join(" -> "))]
    Circular(Vec<String>),
    #[error("{0} is aliased to itself")]
    SelfAlias(String),
    #[error("{key} resolved to a value that is not a {expected}")]
    WrongType { key: String, expected: &'static str },
}

/// Builds a key the container has no entry for (another language's classes, through its reflection).
pub trait Autowirer {
    /// `None` when this autowirer cannot build `key` either.
    fn build(
        &self,
        key: &str,
        container: &Container,
        params: &Params,
    ) -> Option<Result<Resolved, ContainerError>>;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sharing {
    Transient,
    Singleton,
    Scoped,
}

#[derive(Clone)]
enum Kind {
    Binding(Factory, Sharing),
    Instance(Resolved),
}

#[derive(Clone)]
struct Entry {
    kind: Kind,
    lifetime: Lifetime,
    foreign: bool,
}

#[derive(Clone)]
enum Give {
    Factory(Factory),
    Key(String),
}

#[derive(Default)]
struct Inner {
    entries: HashMap<String, Vec<Entry>>,
    /// Changes whenever a key's entries or extenders change; caches built at another generation are stale.
    generation: HashMap<String, u64>,
    shared: HashMap<String, (u64, Resolved)>,
    scoped: HashMap<String, (u64, Resolved)>,
    aliases: HashMap<String, Vec<(String, Lifetime)>>,
    extenders: HashMap<String, Vec<(Extender, Lifetime)>>,
    contextual: HashMap<(String, String), Vec<(Give, Lifetime)>>,
    tags: HashMap<String, Vec<(String, Lifetime)>>,
    before: Vec<(Option<String>, KeyCallback, Lifetime)>,
    resolving: Vec<(Option<String>, ValueCallback, Lifetime)>,
    after: Vec<(Option<String>, ValueCallback, Lifetime)>,
    resolved: HashSet<String>,
    /// Keys resolved since the request started.
    resolved_in_request: HashSet<String>,
}

/// See the crate docs.
pub struct Container {
    inner: RefCell<Inner>,
    building: RefCell<Vec<String>>,
    lifetime: Cell<Lifetime>,
    autowirer: RefCell<Option<Rc<dyn Autowirer>>>,
}

impl Default for Container {
    fn default() -> Self {
        Container::new()
    }
}

/// Returned by [`Container::registering`]: registrations made while it lives have its lifetime.
pub struct Registering<'a> {
    container: &'a Container,
    previous: Lifetime,
}

impl Drop for Registering<'_> {
    fn drop(&mut self) {
        self.container.lifetime.set(self.previous);
    }
}

/// The `when(..)` of a contextual binding.
pub struct When<'a> {
    container: &'a Container,
    consumer: String,
}

/// The `when(..)->needs(..)` of a contextual binding.
pub struct Needs<'a> {
    container: &'a Container,
    consumer: String,
    need: String,
}

impl<'a> When<'a> {
    pub fn needs(self, key: &str) -> Needs<'a> {
        Needs {
            container: self.container,
            consumer: self.consumer,
            need: key.to_string(),
        }
    }
}

impl Needs<'_> {
    /// Builds the dependency with `factory` for this consumer.
    pub fn give(
        self,
        factory: impl Fn(&Container, &Params) -> Result<Resolved, ContainerError> + 'static,
    ) {
        self.container
            .add_contextual(self.consumer, self.need, Give::Factory(Rc::new(factory)));
    }

    /// Resolves another key in its place for this consumer.
    pub fn give_key(self, key: &str) {
        self.container
            .add_contextual(self.consumer, self.need, Give::Key(key.to_string()));
    }
}

impl Container {
    pub fn new() -> Container {
        Container {
            inner: RefCell::new(Inner::default()),
            building: RefCell::new(Vec::new()),
            lifetime: Cell::new(Lifetime::Worker),
            autowirer: RefCell::new(None),
        }
    }

    /// Registrations made while the returned guard lives get `lifetime` (a bridge sets `Request` around PHP's calls).
    pub fn registering(&self, lifetime: Lifetime) -> Registering<'_> {
        let previous = self.lifetime.replace(lifetime);
        Registering {
            container: self,
            previous,
        }
    }

    /// Who builds keys that have no entry.
    pub fn set_autowirer(&self, autowirer: Rc<dyn Autowirer>) {
        *self.autowirer.borrow_mut() = Some(autowirer);
    }

    // ----- binding

    pub fn bind(
        &self,
        key: &str,
        factory: impl Fn(&Container, &Params) -> Result<Resolved, ContainerError> + 'static,
    ) {
        self.push(
            key,
            Kind::Binding(Rc::new(factory), Sharing::Transient),
            false,
        );
    }

    pub fn bind_if(
        &self,
        key: &str,
        factory: impl Fn(&Container, &Params) -> Result<Resolved, ContainerError> + 'static,
    ) {
        if !self.has(key) {
            self.bind(key, factory);
        }
    }

    pub fn singleton(
        &self,
        key: &str,
        factory: impl Fn(&Container, &Params) -> Result<Resolved, ContainerError> + 'static,
    ) {
        self.push(
            key,
            Kind::Binding(Rc::new(factory), Sharing::Singleton),
            false,
        );
    }

    pub fn singleton_if(
        &self,
        key: &str,
        factory: impl Fn(&Container, &Params) -> Result<Resolved, ContainerError> + 'static,
    ) {
        if !self.has(key) {
            self.singleton(key, factory);
        }
    }

    /// Shared until the request ends.
    pub fn scoped(
        &self,
        key: &str,
        factory: impl Fn(&Container, &Params) -> Result<Resolved, ContainerError> + 'static,
    ) {
        self.push(key, Kind::Binding(Rc::new(factory), Sharing::Scoped), false);
    }

    pub fn scoped_if(
        &self,
        key: &str,
        factory: impl Fn(&Container, &Params) -> Result<Resolved, ContainerError> + 'static,
    ) {
        if !self.has(key) {
            self.scoped(key, factory);
        }
    }

    pub fn instance(&self, key: &str, value: Resolved) {
        self.push(key, Kind::Instance(value), false);
    }

    /// A binding made by a language bridge: [`Container::is_foreign`] is true while it is on top.
    pub fn bind_foreign(
        &self,
        key: &str,
        factory: impl Fn(&Container, &Params) -> Result<Resolved, ContainerError> + 'static,
    ) {
        self.push(
            key,
            Kind::Binding(Rc::new(factory), Sharing::Transient),
            true,
        );
    }

    /// As [`Container::bind_foreign`], shared.
    pub fn singleton_foreign(
        &self,
        key: &str,
        factory: impl Fn(&Container, &Params) -> Result<Resolved, ContainerError> + 'static,
    ) {
        self.push(
            key,
            Kind::Binding(Rc::new(factory), Sharing::Singleton),
            true,
        );
    }

    /// As [`Container::bind_foreign`], shared until the request ends.
    pub fn scoped_foreign(
        &self,
        key: &str,
        factory: impl Fn(&Container, &Params) -> Result<Resolved, ContainerError> + 'static,
    ) {
        self.push(key, Kind::Binding(Rc::new(factory), Sharing::Scoped), true);
    }

    /// An instance registered by a language bridge.
    pub fn instance_foreign(&self, key: &str, value: Resolved) {
        self.push(key, Kind::Instance(value), true);
    }

    /// Binds a port (a trait object) by its key.
    pub fn bind_port<P: ?Sized + 'static>(
        &self,
        key: &str,
        factory: impl Fn(&Container, &Params) -> Result<Rc<P>, ContainerError> + 'static,
    ) {
        self.bind(key, move |c, p| {
            factory(c, p).map(|port| Rc::new(port) as Resolved)
        });
    }

    /// As [`Container::bind_port`], shared.
    pub fn singleton_port<P: ?Sized + 'static>(
        &self,
        key: &str,
        factory: impl Fn(&Container, &Params) -> Result<Rc<P>, ContainerError> + 'static,
    ) {
        self.singleton(key, move |c, p| {
            factory(c, p).map(|port| Rc::new(port) as Resolved)
        });
    }

    /// The value of a port binding, as its trait object.
    pub fn make_port<P: ?Sized + 'static>(&self, key: &str) -> Result<Rc<P>, ContainerError> {
        let value = self.make(key)?;
        let port = match value.downcast_ref::<Bridged>() {
            Some(bridged) => bridged.port.downcast_ref::<Rc<P>>().cloned(),
            None => value.downcast_ref::<Rc<P>>().cloned(),
        };
        port.ok_or_else(|| ContainerError::WrongType {
            key: key.to_string(),
            expected: std::any::type_name::<P>(),
        })
    }

    fn push(&self, key: &str, kind: Kind, foreign: bool) {
        let lifetime = self.lifetime.get();
        let mut inner = self.inner.borrow_mut();
        let stack = inner.entries.entry(key.to_string()).or_default();
        let entry = Entry {
            kind,
            lifetime,
            foreign,
        };
        match lifetime {
            // A worker registration replaces the worker entry, under any request ones.
            Lifetime::Worker => {
                stack.retain(|e| e.lifetime != Lifetime::Worker);
                stack.insert(0, entry);
            }
            Lifetime::Request => stack.push(entry),
        }
        Self::bump(&mut inner, key);
    }

    fn bump(inner: &mut Inner, key: &str) {
        *inner.generation.entry(key.to_string()).or_insert(0) += 1;
        inner.shared.remove(key);
        inner.scoped.remove(key);
    }

    // ----- aliases

    /// `alias` resolves to `key`. Panics on an alias cycle (use [`Container::try_alias`] to handle it).
    pub fn alias(&self, key: &str, alias: &str) {
        if let Err(err) = self.try_alias(key, alias) {
            panic!("{err}");
        }
    }

    pub fn try_alias(&self, key: &str, alias: &str) -> Result<(), ContainerError> {
        if self.get_alias(key) == alias || key == alias {
            return Err(ContainerError::SelfAlias(alias.to_string()));
        }
        let lifetime = self.lifetime.get();
        let mut inner = self.inner.borrow_mut();
        let stack = inner.aliases.entry(alias.to_string()).or_default();
        if lifetime == Lifetime::Worker {
            stack.retain(|(_, l)| *l != Lifetime::Worker);
            stack.insert(0, (key.to_string(), lifetime));
        } else {
            stack.push((key.to_string(), lifetime));
        }
        Ok(())
    }

    pub fn is_alias(&self, key: &str) -> bool {
        self.inner
            .borrow()
            .aliases
            .get(key)
            .is_some_and(|s| !s.is_empty())
    }

    /// The key an alias finally resolves to (the key itself when it is no alias).
    pub fn get_alias(&self, key: &str) -> String {
        let inner = self.inner.borrow();
        let mut current = key.to_string();
        let mut seen = HashSet::new();
        while let Some((target, _)) = inner.aliases.get(&current).and_then(|s| s.last()) {
            if !seen.insert(current.clone()) {
                break;
            }
            current = target.clone();
        }
        current
    }

    // ----- extenders, contextual bindings, tags, callbacks

    /// Decorates every value `key` resolves to, from now on; an already shared value is decorated at once.
    pub fn extend(
        &self,
        key: &str,
        extender: impl Fn(Resolved, &Container) -> Result<Resolved, ContainerError> + 'static,
    ) {
        let key = self.get_alias(key);
        let extender: Extender = Rc::new(extender);
        let lifetime = self.lifetime.get();
        let shared = {
            let mut inner = self.inner.borrow_mut();
            inner
                .extenders
                .entry(key.clone())
                .or_default()
                .push((extender.clone(), lifetime));
            inner.shared.get(&key).cloned()
        };
        if let Some((generation, value)) = shared
            && let Ok(extended) = extender(value, self)
        {
            self.inner
                .borrow_mut()
                .shared
                .insert(key, (generation, extended));
        }
    }

    pub fn when(&self, consumer: &str) -> When<'_> {
        When {
            container: self,
            consumer: consumer.to_string(),
        }
    }

    fn add_contextual(&self, consumer: String, need: String, give: Give) {
        let lifetime = self.lifetime.get();
        self.inner
            .borrow_mut()
            .contextual
            .entry((consumer, need))
            .or_default()
            .push((give, lifetime));
    }

    pub fn tag(&self, keys: &[&str], tag: &str) {
        let lifetime = self.lifetime.get();
        let mut inner = self.inner.borrow_mut();
        let list = inner.tags.entry(tag.to_string()).or_default();
        for key in keys {
            list.push(((*key).to_string(), lifetime));
        }
    }

    /// Every key with `tag`, resolved, in tag order.
    pub fn tagged(&self, tag: &str) -> Result<Vec<Resolved>, ContainerError> {
        let keys: Vec<String> = self
            .inner
            .borrow()
            .tags
            .get(tag)
            .map(|list| list.iter().map(|(k, _)| k.clone()).collect())
            .unwrap_or_default();
        keys.iter().map(|key| self.make(key)).collect()
    }

    pub fn before_resolving(
        &self,
        key: Option<&str>,
        callback: impl Fn(&str, &Container) + 'static,
    ) {
        let lifetime = self.lifetime.get();
        self.inner
            .borrow_mut()
            .before
            .push((key.map(str::to_string), Rc::new(callback), lifetime));
    }

    pub fn resolving(&self, key: Option<&str>, callback: impl Fn(&Resolved, &Container) + 'static) {
        let lifetime = self.lifetime.get();
        self.inner.borrow_mut().resolving.push((
            key.map(str::to_string),
            Rc::new(callback),
            lifetime,
        ));
    }

    pub fn after_resolving(
        &self,
        key: Option<&str>,
        callback: impl Fn(&Resolved, &Container) + 'static,
    ) {
        let lifetime = self.lifetime.get();
        self.inner
            .borrow_mut()
            .after
            .push((key.map(str::to_string), Rc::new(callback), lifetime));
    }

    // ----- resolving

    /// True when `key` (or what it aliases) has an entry.
    pub fn has(&self, key: &str) -> bool {
        let key = self.get_alias(key);
        self.inner
            .borrow()
            .entries
            .get(&key)
            .is_some_and(|s| !s.is_empty())
    }

    /// The same as [`Container::has`] (Laravel's `bound`).
    pub fn bound(&self, key: &str) -> bool {
        self.has(key)
    }

    /// Whether `key` has been resolved at least once.
    pub fn resolved(&self, key: &str) -> bool {
        let key = self.get_alias(key);
        self.inner.borrow().resolved.contains(&key)
    }

    /// Whether the entry answering for `key` now was registered by a language bridge.
    pub fn is_foreign(&self, key: &str) -> bool {
        let key = self.get_alias(key);
        self.inner
            .borrow()
            .entries
            .get(&key)
            .and_then(|s| s.last())
            .is_some_and(|e| e.foreign)
    }

    pub fn make(&self, key: &str) -> Result<Resolved, ContainerError> {
        self.resolve(key, &Params::new())
    }

    /// Builds `key` with `params`; never returns nor becomes a shared instance when `params` is not empty.
    pub fn make_with(&self, key: &str, params: &Params) -> Result<Resolved, ContainerError> {
        self.resolve(key, params)
    }

    /// A function that makes `key` each time it is called.
    pub fn factory<'a>(&'a self, key: &str) -> impl Fn() -> Result<Resolved, ContainerError> + 'a {
        let key = key.to_string();
        move || self.make(&key)
    }

    fn resolve(&self, key: &str, params: &Params) -> Result<Resolved, ContainerError> {
        let value = self.resolve_entry(key, params)?;
        let key = self.get_alias(key);
        self.inner.borrow_mut().resolved_in_request.insert(key);
        Ok(value)
    }

    /// Whether `key` was resolved since the request started (a cached shared value counts).
    pub fn resolved_in_request(&self, key: &str) -> bool {
        let key = self.get_alias(key);
        self.inner.borrow().resolved_in_request.contains(&key)
    }

    /// Starts a request: nothing counts as resolved in it yet.
    pub fn start_request(&self) {
        self.inner.borrow_mut().resolved_in_request.clear();
    }

    fn resolve_entry(&self, key: &str, params: &Params) -> Result<Resolved, ContainerError> {
        let requested = key.to_string();
        let key = self.get_alias(key);

        // A contextual binding of the consumer being built now.
        let consumer = self.building.borrow().last().cloned();
        if let Some(consumer) = consumer {
            let give = self
                .inner
                .borrow()
                .contextual
                .get(&(consumer, key.clone()))
                .and_then(|list| list.last())
                .map(|(give, _)| give.clone());
            match give {
                Some(Give::Key(other)) => return self.resolve(&other, params),
                Some(Give::Factory(factory)) => return self.build(&key, &factory, params),
                None => {}
            }
        }

        if self.building.borrow().contains(&key) {
            let mut chain = self.building.borrow().clone();
            chain.push(key);
            return Err(ContainerError::Circular(chain));
        }

        let (entry, generation) = {
            let inner = self.inner.borrow();
            (
                inner.entries.get(&key).and_then(|s| s.last()).cloned(),
                inner.generation.get(&key).copied().unwrap_or(0),
            )
        };
        let Some(entry) = entry else {
            let autowirer = self.autowirer.borrow().clone();
            if let Some(autowirer) = autowirer {
                self.building.borrow_mut().push(key.clone());
                let built = autowirer.build(&key, self, params);
                self.building.borrow_mut().pop();
                if let Some(built) = built {
                    return built.and_then(|value| self.decorate(&key, value));
                }
            }
            return Err(ContainerError::Unresolvable(
                requested,
                "nothing is bound to it, and nothing could build it".to_string(),
            ));
        };

        match entry.kind {
            Kind::Instance(value) => {
                self.inner.borrow_mut().resolved.insert(key.clone());
                self.apply_extenders(&key, value)
            }
            Kind::Binding(factory, sharing) => {
                let fresh = !params.is_empty();
                if !fresh {
                    let cached = {
                        let inner = self.inner.borrow();
                        let cache = match sharing {
                            Sharing::Singleton => inner.shared.get(&key),
                            Sharing::Scoped => inner.scoped.get(&key),
                            Sharing::Transient => None,
                        };
                        cache
                            .filter(|(g, _)| *g == generation)
                            .map(|(_, v)| v.clone())
                    };
                    if let Some(value) = cached {
                        return Ok(value);
                    }
                }
                let value = self.build(&key, &factory, params)?;
                if !fresh {
                    let mut inner = self.inner.borrow_mut();
                    match sharing {
                        Sharing::Singleton => {
                            inner
                                .shared
                                .insert(key.clone(), (generation, value.clone()));
                        }
                        Sharing::Scoped => {
                            inner
                                .scoped
                                .insert(key.clone(), (generation, value.clone()));
                        }
                        Sharing::Transient => {}
                    }
                }
                Ok(value)
            }
        }
    }

    /// Runs the factory with `key` marked as being built, then the extenders and the resolving callbacks.
    fn build(
        &self,
        key: &str,
        factory: &Factory,
        params: &Params,
    ) -> Result<Resolved, ContainerError> {
        self.run_key_callbacks(key);
        self.building.borrow_mut().push(key.to_string());
        let built = factory(self, params);
        self.building.borrow_mut().pop();
        let value = built?;
        self.decorate(key, value)
    }

    fn decorate(&self, key: &str, value: Resolved) -> Result<Resolved, ContainerError> {
        let value = self.apply_extenders(key, value)?;
        self.inner.borrow_mut().resolved.insert(key.to_string());
        self.run_value_callbacks(key, &value);
        Ok(value)
    }

    fn apply_extenders(&self, key: &str, mut value: Resolved) -> Result<Resolved, ContainerError> {
        let extenders: Vec<Extender> = self
            .inner
            .borrow()
            .extenders
            .get(key)
            .map(|list| list.iter().map(|(e, _)| e.clone()).collect())
            .unwrap_or_default();
        for extender in extenders {
            value = extender(value, self)?;
        }
        Ok(value)
    }

    fn run_key_callbacks(&self, key: &str) {
        let callbacks: Vec<KeyCallback> = Self::matching(&self.inner.borrow().before, key);
        for callback in callbacks {
            callback(key, self);
        }
    }

    fn run_value_callbacks(&self, key: &str, value: &Resolved) {
        let (resolving, after) = {
            let inner = self.inner.borrow();
            (
                Self::matching(&inner.resolving, key),
                Self::matching(&inner.after, key),
            )
        };
        for callback in resolving.into_iter().chain(after) {
            callback(value, self);
        }
    }

    /// The callbacks for `key` first, then the global ones, each in registration order.
    fn matching<F: Clone>(list: &[(Option<String>, F, Lifetime)], key: &str) -> Vec<F> {
        let specific = list
            .iter()
            .filter(|(k, _, _)| k.as_deref() == Some(key))
            .map(|(_, f, _)| f.clone());
        let global = list
            .iter()
            .filter(|(k, _, _)| k.is_none())
            .map(|(_, f, _)| f.clone());
        specific.chain(global).collect()
    }

    // ----- forgetting

    /// Drops the shared value of `key` (and its registered instances).
    pub fn forget_instance(&self, key: &str) {
        let key = self.get_alias(key);
        let mut inner = self.inner.borrow_mut();
        inner.shared.remove(&key);
        if let Some(stack) = inner.entries.get_mut(&key) {
            stack.retain(|e| !matches!(e.kind, Kind::Instance(_)));
        }
        Self::bump(&mut inner, &key);
    }

    /// Drops every shared value and registered instance.
    pub fn forget_instances(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.shared.clear();
        let keys: Vec<String> = inner.entries.keys().cloned().collect();
        for key in &keys {
            if let Some(stack) = inner.entries.get_mut(key) {
                stack.retain(|e| !matches!(e.kind, Kind::Instance(_)));
            }
            Self::bump(&mut inner, key);
        }
    }

    /// Drops every scoped value; singletons stay.
    pub fn forget_scoped_instances(&self) {
        self.inner.borrow_mut().scoped.clear();
    }

    /// Forgets everything.
    pub fn flush(&self) {
        *self.inner.borrow_mut() = Inner::default();
    }

    /// Ends the request: removes every `Request` registration (bindings, instances, aliases, extenders, contextual
    /// bindings, tags, callbacks) and every scoped value. Each key answers with its worker-lifetime entry again.
    pub fn end_request(&self) {
        let mut inner = self.inner.borrow_mut();
        let keys: Vec<String> = inner.entries.keys().cloned().collect();
        for key in &keys {
            let mut changed = false;
            if let Some(stack) = inner.entries.get_mut(key) {
                let before = stack.len();
                stack.retain(|e| e.lifetime == Lifetime::Worker);
                changed = stack.len() != before;
            }
            if changed {
                Self::bump(&mut inner, key);
            }
        }
        let extended: Vec<String> = inner
            .extenders
            .iter()
            .filter(|(_, list)| list.iter().any(|(_, l)| *l == Lifetime::Request))
            .map(|(k, _)| k.clone())
            .collect();
        for list in inner.extenders.values_mut() {
            list.retain(|(_, l)| *l == Lifetime::Worker);
        }
        for key in &extended {
            Self::bump(&mut inner, key);
        }
        for list in inner.aliases.values_mut() {
            list.retain(|(_, l)| *l == Lifetime::Worker);
        }
        for list in inner.contextual.values_mut() {
            list.retain(|(_, l)| *l == Lifetime::Worker);
        }
        for list in inner.tags.values_mut() {
            list.retain(|(_, l)| *l == Lifetime::Worker);
        }
        inner.before.retain(|(_, _, l)| *l == Lifetime::Worker);
        inner.resolving.retain(|(_, _, l)| *l == Lifetime::Worker);
        inner.after.retain(|(_, _, l)| *l == Lifetime::Worker);
        inner.scoped.clear();
        inner.resolved_in_request.clear();
    }
}
