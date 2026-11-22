//! The native functions behind the facade: each resolves its port from the container on every call, so a PHP
//! replacement bound for the request answers instead of WordPress's rules.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_hooks::Value;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_crypto_sodium::Sodium;
use speedyspec_wp_password::{
    application_password_check, application_password_hash, chunk_password,
};
use speedyspec_wp_password_contracts::{
    PASSWORD_CHECKER, PASSWORD_GENERATOR, PASSWORD_HASHER, PasswordChecker, PasswordError,
    PasswordGenerator, PasswordHasher, RANDOM_NUMBER, REHASH_POLICY, RandomNumber, RehashPolicy,
};

use crate::adapters::php_string;

const NATIVE: &str = "SpeedySpec\\WpPassword\\Native\\";

/// What the native functions share: the container the ports are bound in, and libsodium for application passwords.
pub(crate) struct Natives {
    pub(crate) container: Rc<Container>,
    pub(crate) sodium: Rc<Sodium>,
}

impl Natives {
    fn port<P: ?Sized + 'static>(&self, key: &str) -> Result<Rc<P>, NativeError> {
        self.container
            .make_port::<P>(key)
            .map_err(|err| NativeError::throw("Error", err.to_string()))
    }
}

/// A password error as PHP sees it.
pub(crate) fn native_error(err: PasswordError) -> NativeError {
    match err {
        PasswordError::Throw { class, message } => NativeError::throw(&class, message),
        PasswordError::Rethrow(exception) => NativeError::Rethrow(exception),
        PasswordError::Unwinding => NativeError::Silent,
        PasswordError::Port(why) => NativeError::throw("Error", why),
    }
}

/// Argument `i` as bytes; the facade has already made it a string.
fn bytes(args: &[Value], i: usize, function: &str) -> Result<Vec<u8>, NativeError> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.as_bytes().to_vec()),
        Some(Value::Bytes(b)) => Ok(b.clone()),
        other => Err(NativeError::throw(
            "TypeError",
            format!(
                "{NATIVE}{function}(): Argument #{} must be of type string, {other:?} given",
                i + 1
            ),
        )),
    }
}

/// Argument `i` as an integer; the facade has already cast it.
fn int(args: &[Value], i: usize, function: &str) -> Result<i64, NativeError> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n),
        other => Err(NativeError::throw(
            "TypeError",
            format!(
                "{NATIVE}{function}(): Argument #{} must be of type int, {other:?} given",
                i + 1
            ),
        )),
    }
}

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

pub(crate) fn functions(natives: &Rc<Natives>) -> Vec<FunctionBuilder<'static>> {
    let n: Vec<Rc<Natives>> = (0..8).map(|_| natives.clone()).collect();
    let [a, b, c, d, e, f, g, _h]: [Rc<Natives>; 8] = n.try_into().ok().expect("eight handles");
    vec![
        native::function(&format!("{NATIVE}wp_hash_password"), move |args| {
            let password = bytes(args, 0, "wp_hash_password")?;
            a.port::<dyn PasswordHasher>(PASSWORD_HASHER)?
                .hash(&password)
                .map_err(native_error)
        }),
        native::function(&format!("{NATIVE}wp_check_password"), move |args| {
            let password = bytes(args, 0, "wp_check_password")?;
            let hash = bytes(args, 1, "wp_check_password")?;
            b.port::<dyn PasswordChecker>(PASSWORD_CHECKER)?
                .check(&password, &hash, &arg(args, 2))
                .map_err(native_error)
        }),
        native::function(&format!("{NATIVE}wp_password_needs_rehash"), move |args| {
            let hash = bytes(args, 0, "wp_password_needs_rehash")?;
            c.port::<dyn RehashPolicy>(REHASH_POLICY)?
                .needs_rehash(&hash, &arg(args, 1))
                .map_err(native_error)
        }),
        native::function(&format!("{NATIVE}wp_generate_password"), move |args| {
            d.port::<dyn PasswordGenerator>(PASSWORD_GENERATOR)?
                .generate(&arg(args, 0), &arg(args, 1), &arg(args, 2))
                .map_err(native_error)
        }),
        native::function(&format!("{NATIVE}wp_rand"), move |args| {
            let (min, max) = (int(args, 0, "wp_rand")?, int(args, 1, "wp_rand")?);
            e.port::<dyn RandomNumber>(RANDOM_NUMBER)?
                .rand(min, max)
                .map_err(native_error)
        }),
        native::function(&format!("{NATIVE}application_password_hash"), move |args| {
            let password = bytes(args, 0, "application_password_hash")?;
            Ok(Value::Str(application_password_hash(
                &*f.sodium, &*f.sodium, &password,
            )))
        }),
        native::function(
            &format!("{NATIVE}application_password_check"),
            move |args| {
                let password = bytes(args, 0, "application_password_check")?;
                let hash = bytes(args, 1, "application_password_check")?;
                let checker = g.port::<dyn PasswordChecker>(PASSWORD_CHECKER)?;
                application_password_check(&*checker, &*g.sodium, &*g.sodium, &password, &hash)
                    .map(Value::Bool)
                    .map_err(native_error)
            },
        ),
        native::function(&format!("{NATIVE}chunk_password"), move |args| {
            Ok(php_string(&chunk_password(&bytes(
                args,
                0,
                "chunk_password",
            )?)))
        }),
    ]
}
