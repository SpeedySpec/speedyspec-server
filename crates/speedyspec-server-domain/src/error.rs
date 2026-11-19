//! Everything a domain value can reject.

/// A value that breaks a domain rule. Every message names the bad value.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DomainError {
    #[error(
        "app id {0:?} must start with a lowercase letter and contain only lowercase letters, digits and hyphens"
    )]
    BadAppId(String),
    #[error("release index is not valid: {0}")]
    BadIndex(String),
    #[error("release manifest is not valid: {0}")]
    BadManifest(String),
    #[error("a {kind} release must have a `{section}` section")]
    MissingSection {
        kind: &'static str,
        section: &'static str,
    },
    #[error("a {kind} release must not have a `{section}` section")]
    UnexpectedSection {
        kind: &'static str,
        section: &'static str,
    },
    #[error("static.root {0:?} must be a relative path without `..`")]
    BadStaticRoot(String),
    #[error("php.{field} {path:?} must be a relative path without `..`")]
    BadPhpPath { field: &'static str, path: String },
    #[error("mount path {0:?} must start and end with `/`")]
    BadMountPath(String),
    #[error("mount host {0:?} must be lowercase")]
    BadMountHost(String),
    #[error("two mounts use host {host} and path {path}")]
    DuplicateMount { host: String, path: String },
}
