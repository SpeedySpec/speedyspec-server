//! One set of registries per site, per worker.
//!
//! A site is a blog plus a fingerprint of its active plugins and theme. Its registries are filled on the first request
//! that needs operations (by `wp_operations_init`), closed, and kept for the worker. A new fingerprint for the same
//! blog replaces that blog's site, which is how registries are rebuilt when plugins or the theme change. Nothing is
//! shared across replicas.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_operation::{RegistrationError, Registries};
use speedyspec_operation_dispatch::Dispatcher;
use speedyspec_php_ext::Value as PhpValue;

/// Which site a request serves.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SiteId {
    pub blog: i64,
    pub fingerprint: String,
}

/// Port: the site the current request serves.
pub trait SiteKey {
    fn current(&self) -> SiteId;
}

/// Always the same site (tests, single-site hosts that rebuild on deploy).
pub struct FixedSiteKey(pub SiteId);

impl SiteKey for FixedSiteKey {
    fn current(&self) -> SiteId {
        self.0.clone()
    }
}

/// The site PHP reports (`Internal\SiteKey::current()`): the current blog, and its active plugins and theme.
pub struct PhpSiteKey;

impl SiteKey for PhpSiteKey {
    fn current(&self) -> SiteId {
        let reported = speedyspec_php::call(
            &PhpValue::Str("SpeedySpec\\Operation\\Internal\\SiteKey::current".into()),
            &[],
        );
        let fallback = SiteId {
            blog: 1,
            fingerprint: String::new(),
        };
        let Ok(PhpValue::Array(items)) = reported else {
            eprintln!("speedyspec_wordpress: reading the operation site key failed: {reported:?}");
            return fallback;
        };
        match items.as_slice() {
            [(_, PhpValue::Int(blog)), (_, PhpValue::Str(fingerprint))] => SiteId {
                blog: *blog,
                fingerprint: fingerprint.clone(),
            },
            _ => fallback,
        }
    }
}

/// One site's registries and the dispatcher over them.
pub struct Site {
    pub id: SiteId,
    pub registries: Rc<Registries>,
    pub dispatcher: Rc<Dispatcher>,
    problems: RefCell<Vec<RegistrationError>>,
}

impl Site {
    fn new(id: SiteId, container: Rc<Container>) -> Site {
        let registries = Rc::new(Registries::new());
        Site {
            id,
            dispatcher: Rc::new(Dispatcher::new(container, registries.clone())),
            registries,
            problems: RefCell::new(Vec::new()),
        }
    }

    /// What closing the registries dropped, and why.
    pub fn problems(&self) -> Vec<RegistrationError> {
        self.problems.borrow().clone()
    }

    /// Closes the registries if they are open, keeping (and logging) what the close dropped.
    pub(crate) fn close(&self) {
        if self.registries.is_closed() {
            return;
        }
        let errors = self.registries.close_lenient();
        for error in &errors {
            eprintln!("speedyspec_wordpress: operation registration dropped a definition: {error}");
        }
        self.problems.borrow_mut().extend(errors);
    }
}

/// Every site this worker serves, one per blog.
pub struct Sites {
    container: Rc<Container>,
    by_blog: RefCell<HashMap<i64, Rc<Site>>>,
    initialized: RefCell<HashSet<SiteId>>,
}

impl Sites {
    pub fn new(container: Rc<Container>) -> Sites {
        Sites {
            container,
            by_blog: RefCell::new(HashMap::new()),
            initialized: RefCell::new(HashSet::new()),
        }
    }

    /// The site `id`, created (and replacing that blog's previous site) when new. Fires nothing.
    pub fn registering(&self, id: &SiteId) -> Rc<Site> {
        if let Some(site) = self.by_blog.borrow().get(&id.blog)
            && site.id == *id
        {
            return site.clone();
        }
        let site = Rc::new(Site::new(id.clone(), self.container.clone()));
        self.by_blog.borrow_mut().insert(id.blog, site.clone());
        site
    }

    /// The sites this worker holds, one per blog, ordered by blog.
    pub fn site_ids(&self) -> Vec<SiteId> {
        let mut ids: Vec<SiteId> = self
            .by_blog
            .borrow()
            .values()
            .map(|s| s.id.clone())
            .collect();
        ids.sort_by_key(|id| id.blog);
        ids
    }

    /// Marks `id` initialized for this request; true the first time.
    pub(crate) fn mark_initialized(&self, id: &SiteId) -> bool {
        self.initialized.borrow_mut().insert(id.clone())
    }

    /// Forgets this request's initializations and discards sites left open (a fatal error mid-registration), so the
    /// next request rebuilds them.
    pub fn end_request(&self) {
        self.initialized.borrow_mut().clear();
        self.by_blog
            .borrow_mut()
            .retain(|_, site| site.registries.is_closed());
    }
}
