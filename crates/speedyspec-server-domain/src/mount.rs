//! Mounts: where an app is reachable, by host and path prefix.

use serde::{Deserialize, Serialize};

use crate::{AppId, DomainError};

/// Which hosts a mount answers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HostMatch
{
    /// Any host (`*` in configuration).
    Any,
    /// One host, lowercase, without a port.
    Exact(String),
}

/// Who may reach a mount.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Access
{
    Public,
    SignedIn,
}

/// An app reachable at a host and path prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount
{
    pub host: HostMatch,
    /// Starts and ends with `/`.
    pub path: String,
    pub app: AppId,
    pub access: Access,
}

/// Every mount, checked, ready to resolve requests.
#[derive(Clone, Debug)]
pub struct MountTable
{
    mounts: Vec<Mount>,
}

impl MountTable
{
    /// Checks every mount's path and host, and that no two mounts share a host and path.
    pub fn new(mounts: Vec<Mount>) -> Result<MountTable, DomainError>
    {
        for (index, mount) in mounts.iter().enumerate()
        {
            if !mount.path.starts_with('/') || !mount.path.ends_with('/')
            {
                return Err(DomainError::BadMountPath(mount.path.clone()));
            }
            if let HostMatch::Exact(host) = &mount.host
                && (host.is_empty() || host.chars().any(|c| c.is_ascii_uppercase()))
            {
                return Err(DomainError::BadMountHost(host.clone()));
            }
            if mounts[..index].iter().any(|other| other.host == mount.host && other.path == mount.path)
            {
                let host = match &mount.host
                {
                    HostMatch::Any => "*".to_string(),
                    HostMatch::Exact(host) => host.clone(),
                };
                return Err(DomainError::DuplicateMount { host, path: mount.path.clone() });
            }
        }

        Ok(MountTable { mounts })
    }

    /// The mount for an already-normalized host and path: among mounts for exactly this host, the longest matching
    /// prefix; only when none matches, the longest matching prefix among `*` mounts.
    pub fn resolve(&self, host: &str, path: &str) -> Option<&Mount>
    {
        let longest = |exact: bool| {
            self.mounts
                .iter()
                .filter(|mount| match &mount.host
                {
                    HostMatch::Exact(name) => exact && name == host,
                    HostMatch::Any => !exact,
                })
                .filter(|mount| path.starts_with(&mount.path))
                .max_by_key(|mount| mount.path.len())
        };

        longest(true).or_else(|| longest(false))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Mount>
    {
        self.mounts.iter()
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    fn mount(host: &str, path: &str, app: &str) -> Mount
    {
        Mount {
            host: if host == "*" { HostMatch::Any } else { HostMatch::Exact(host.to_string()) },
            path: path.to_string(),
            app: AppId::parse(app).unwrap(),
            access: Access::Public,
        }
    }

    #[test]
    fn mount_table_rejects()
    {
        assert!(MountTable::new(vec![mount("*", "admin/", "admin")]).is_err());
        assert!(MountTable::new(vec![mount("*", "/admin", "admin")]).is_err());
        assert!(MountTable::new(vec![mount("*", "/admin/", "admin"), mount("*", "/admin/", "blog")]).is_err());
        assert!(MountTable::new(vec![mount("Example.com", "/", "blog")]).is_err());
    }

    #[test]
    fn mount_resolve_longest_prefix()
    {
        let table = MountTable::new(vec![
            mount("*", "/", "blog"),
            mount("*", "/admin/", "admin"),
            mount("*", "/admin/api/", "server"),
        ])
        .unwrap();
        assert_eq!(table.resolve("example.com", "/admin/api/x").unwrap().app.as_str(), "server");
        assert_eq!(table.resolve("example.com", "/admin/x").unwrap().app.as_str(), "admin");
        assert_eq!(table.resolve("example.com", "/blog").unwrap().app.as_str(), "blog");
    }

    #[test]
    fn mount_resolve_exact_host_beats_any()
    {
        let table = MountTable::new(vec![mount("example.com", "/", "blog"), mount("*", "/admin/", "admin")]).unwrap();
        assert_eq!(table.resolve("example.com", "/admin/x").unwrap().app.as_str(), "blog");
        assert_eq!(table.resolve("other.org", "/admin/x").unwrap().app.as_str(), "admin");
    }

    #[test]
    fn mount_resolve_none()
    {
        let table = MountTable::new(vec![mount("*", "/admin/", "admin")]).unwrap();
        assert!(table.resolve("example.com", "/x").is_none());
    }

    #[test]
    fn access_is_kebab_case()
    {
        assert_eq!(serde_json::from_str::<Access>("\"signed-in\"").unwrap(), Access::SignedIn);
        assert_eq!(serde_json::from_str::<Access>("\"public\"").unwrap(), Access::Public);
    }
}
