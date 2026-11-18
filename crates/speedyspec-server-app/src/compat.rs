//! Whether a set of releases works together, by each release's declared requirements.

use std::collections::BTreeMap;
use std::fmt;

use semver::{Version, VersionReq};
use speedyspec_server_domain::{AppId, ReleaseManifest};

/// One release's requirement that another release in the set does not meet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incompatibility
{
    pub app: AppId,
    pub version: Version,
    pub requires: AppId,
    pub range: VersionReq,
    pub actual: Version,
}

impl fmt::Display for Incompatibility
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        write!(
            f,
            "{} {} requires {} {}, active {} is {}",
            self.app, self.version, self.requires, self.range, self.requires, self.actual
        )
    }
}

/// Checks every release's requirements against the others in `set`. A requirement on an app that is not in the set is
/// ignored, and logged once at `info` per call, so a release can name an app a deployment does not run.
pub fn check_set(set: &BTreeMap<AppId, ReleaseManifest>) -> Result<(), Vec<Incompatibility>>
{
    let mut failures = Vec::new();
    let mut ignored = Vec::new();
    for manifest in set.values()
    {
        for (target, range) in &manifest.requires
        {
            match set.get(target)
            {
                None => ignored.push(format!("{} requires {target}", manifest.app)),
                Some(other) if !range.matches(&other.version) => failures.push(Incompatibility {
                    app: manifest.app.clone(),
                    version: manifest.version.clone(),
                    requires: target.clone(),
                    range: range.clone(),
                    actual: other.version.clone(),
                }),
                Some(_) => {}
            }
        }
    }
    if !ignored.is_empty()
    {
        tracing::info!(requirements = ?ignored, "ignoring requirements on apps that are not configured");
    }
    if failures.is_empty()
    {
        return Ok(());
    }
    failures.sort_by(|a, b| (&a.app, &a.requires).cmp(&(&b.app, &b.requires)));

    Err(failures)
}

#[cfg(test)]
mod tests
{
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use speedyspec_server_domain::AppKind;

    use super::*;

    fn manifest(app: &str, version: &str, requires: &[(&str, &str)]) -> ReleaseManifest
    {
        ReleaseManifest {
            app: AppId::parse(app).unwrap(),
            version: Version::parse(version).unwrap(),
            kind: if app == "server" { AppKind::Server } else { AppKind::Php },
            requires: requires
                .iter()
                .map(|(id, range)| (AppId::parse(id).unwrap(), VersionReq::parse(range).unwrap()))
                .collect(),
            static_: None,
            php: None,
        }
    }

    fn set(manifests: Vec<ReleaseManifest>) -> BTreeMap<AppId, ReleaseManifest>
    {
        manifests.into_iter().map(|m| (m.app.clone(), m)).collect()
    }

    #[test]
    fn consistent_set_passes()
    {
        let releases = set(vec![manifest("server", "3.2.1", &[]), manifest("admin", "5.2.0", &[("server", "^3.1")])]);
        assert_eq!(check_set(&releases), Ok(()));
    }

    #[test]
    fn reports_unmet_requirement()
    {
        let releases = set(vec![manifest("server", "3.2.1", &[]), manifest("admin", "6.0.0", &[("server", "^4")])]);
        let failures = check_set(&releases).unwrap_err();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].to_string(), "admin 6.0.0 requires server ^4, active server is 3.2.1");
    }

    #[test]
    fn reports_both_directions()
    {
        let releases = set(vec![
            manifest("server", "4.0.0", &[("admin", "^6")]),
            manifest("admin", "5.2.0", &[("server", "^3")]),
        ]);
        let failures = check_set(&releases).unwrap_err();
        let apps: Vec<_> = failures.iter().map(|f| f.app.as_str()).collect();
        assert_eq!(apps, ["admin", "server"]);
    }

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured
    {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize>
        {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()>
        {
            Ok(())
        }
    }

    #[test]
    fn ignores_unconfigured_app()
    {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish();
        let releases = set(vec![manifest("admin", "1.0.0", &[("frontend-api", "^2")])]);

        let result = tracing::subscriber::with_default(subscriber, || check_set(&releases));

        assert_eq!(result, Ok(()));
        let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert_eq!(log.lines().count(), 1, "one event per call: {log}");
        assert!(log.contains("INFO") && log.contains("admin") && log.contains("frontend-api"), "{log}");
    }
}
