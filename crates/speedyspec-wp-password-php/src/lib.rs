//! WordPress's password functions as native PHP functions, over container ports (Facade, Adapter, Plugin).
//!
//! WordPress makes `wp_hash_password()`, `wp_check_password()`, `wp_password_needs_rehash()`,
//! `wp_generate_password()` and `wp_rand()` pluggable: a plugin replaces one by declaring it first. SpeedySpec
//! replaces that with the service container. [`PasswordArea::bind_defaults`] binds WordPress's rules
//! (`speedyspec-wp-password`, over libsodium and bcrypt) under each function's port; [`PasswordArea::ports`] lets a PHP
//! object implementing a port's twin interface (`SpeedySpec\WpPassword\PasswordHasher` and the others) replace it
//! for a request, for PHP and Rust callers alike.
//!
//! The facade (`password.php`) declares the global functions at the start of every request, each inside a
//! `function_exists` guard, plus `SpeedySpec\WpPassword\application_password_hash/check()` and `chunk_password()`.
//! Each resolves its port per call. WordPress's filters run through the Hook API and `$wp_hasher` is honoured.
//!
//! Answers that differ from WordPress on purpose: argon2id is the default; raw MD5, phpass, `$2x$` and `crypt()`-only
//! hashes never verify; `password_hash()`'s ignored-salt warning is an `E_USER_WARNING`.

mod adapters;
mod natives;
mod twins;

use std::rc::{Rc, Weak};

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_crypto_sodium::Sodium;
use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_password::{
    DefaultPasswordChecker, DefaultPasswordGenerator, DefaultPasswordHasher, DefaultRandomNumber,
    DefaultRehashPolicy, PasswordPorts,
};
use speedyspec_wp_password_bcrypt::BcryptCrate;
use speedyspec_wp_password_contracts::{
    PASSWORD_CHECKER, PASSWORD_GENERATOR, PASSWORD_HASHER, PasswordChecker, PasswordError,
    PasswordGenerator, PasswordHasher, RANDOM_NUMBER, REHASH_POLICY, RandomNumber, RehashPolicy,
};

pub use adapters::{HookPasswordFilters, PhpReplacement, PhpWarnings, PhpWpHasherGlobal};
pub use twins::{
    PasswordCheckerTwin, PasswordGeneratorTwin, PasswordHasherTwin, RandomNumberTwin,
    RehashPolicyTwin,
};

const FACADE: &str = include_str!("password.php");

/// The native area of WordPress's password functions.
pub struct PasswordArea {
    natives: Rc<natives::Natives>,
    hooks: Rc<HookApi>,
}

impl PasswordArea {
    /// The area, filtering through `hooks`, resolving ports from `container`, hashing argon2 and drawing random bytes
    /// with `sodium`.
    pub fn new(hooks: Rc<HookApi>, container: Rc<Container>, sodium: Rc<Sodium>) -> PasswordArea {
        PasswordArea {
            natives: Rc::new(natives::Natives { container, sodium }),
            hooks,
        }
    }

    /// Binds WordPress's rules under the five ports, for the life of the worker.
    ///
    /// The bindings reach the Hook API weakly (see [`HookPasswordFilters`]); this area keeps it alive, so filters run
    /// while the area lives.
    pub fn bind_defaults(&self) {
        let container = &self.natives.container;
        let sodium = self.natives.sodium.clone();
        let ports = Rc::new(PasswordPorts {
            filters: Rc::new(HookPasswordFilters::new(&self.hooks)),
            hasher_global: Rc::new(PhpWpHasherGlobal),
            bcrypt: Rc::new(BcryptCrate),
            argon2: sodium.clone(),
            random: sodium.clone(),
            warnings: Rc::new(PhpWarnings),
        });
        let p = ports.clone();
        container.bind_port::<dyn PasswordHasher>(PASSWORD_HASHER, move |_, _| {
            Ok(Rc::new(DefaultPasswordHasher::new(p.clone())) as Rc<dyn PasswordHasher>)
        });
        let p = ports.clone();
        container.bind_port::<dyn PasswordChecker>(PASSWORD_CHECKER, move |_, _| {
            Ok(Rc::new(DefaultPasswordChecker::new(p.clone())) as Rc<dyn PasswordChecker>)
        });
        let p = ports.clone();
        container.bind_port::<dyn RehashPolicy>(REHASH_POLICY, move |_, _| {
            Ok(Rc::new(DefaultRehashPolicy::new(p.clone())) as Rc<dyn RehashPolicy>)
        });
        container.bind_port::<dyn RandomNumber>(RANDOM_NUMBER, move |_, _| {
            Ok(Rc::new(DefaultRandomNumber::new(sodium.clone())) as Rc<dyn RandomNumber>)
        });
        let weak: Weak<Container> = Rc::downgrade(container);
        let filters = ports.filters.clone();
        container.bind_port::<dyn PasswordGenerator>(PASSWORD_GENERATOR, move |_, _| {
            let weak = weak.clone();
            let resolve = Rc::new(move || {
                weak.upgrade()
                    .ok_or_else(|| PasswordError::Port("the container is gone".into()))?
                    .make_port::<dyn RandomNumber>(RANDOM_NUMBER)
                    .map_err(|err| PasswordError::Port(err.to_string()))
            });
            Ok(
                Rc::new(DefaultPasswordGenerator::new(resolve, filters.clone()))
                    as Rc<dyn PasswordGenerator>,
            )
        });
    }

    /// `area` with the five ports registered, so a PHP object bound to one reaches Rust callers as a
    /// [`PhpReplacement`].
    pub fn ports(area: ContainerArea) -> ContainerArea {
        area.port::<dyn PasswordHasher>(PASSWORD_HASHER, |object| {
            Rc::new(PhpReplacement::new(object))
        })
        .port::<dyn PasswordChecker>(PASSWORD_CHECKER, |object| {
            Rc::new(PhpReplacement::new(object))
        })
        .port::<dyn RehashPolicy>(REHASH_POLICY, |object| Rc::new(PhpReplacement::new(object)))
        .port::<dyn PasswordGenerator>(PASSWORD_GENERATOR, |object| {
            Rc::new(PhpReplacement::new(object))
        })
        .port::<dyn RandomNumber>(RANDOM_NUMBER, |object| Rc::new(PhpReplacement::new(object)))
    }
}

impl NativeArea for PasswordArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        natives::functions(&self.natives)
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-password", FACADE) {
            eprintln!(
                "speedyspec_wordpress: installing WordPress's password functions failed: {err}"
            );
        }
    }
}
