//! What each app kind does when one of its releases goes live, in the running server.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use speedyspec_server_domain::{AppId, AppRuntime, Prepared, ReleaseManifest};
use speedyspec_server_presentation::{AppHandle, Fallback, LiveApps, StaticApp};
use speedyspec_server_static::StaticSet;

/// Static releases: prepare loads the whole set into memory; activate swaps it in, keeping the old set's immutable
/// files as a fallback for `grace`.
pub struct StaticRuntime {
    live: Arc<LiveApps>,
    limit: u64,
    grace: Duration,
}

impl StaticRuntime {
    pub fn new(live: Arc<LiveApps>, limit: u64, grace: Duration) -> StaticRuntime {
        StaticRuntime { live, limit, grace }
    }
}

impl AppRuntime for StaticRuntime {
    fn prepare(
        &self,
        app: &AppId,
        manifest: &ReleaseManifest,
        dir: &Path,
    ) -> Result<Prepared, String> {
        let section = manifest
            .static_
            .as_ref()
            .ok_or("the release has no static section")?;
        let set = StaticSet::build(dir, section, self.limit).map_err(|err| err.to_string())?;
        tracing::info!(event = "static_loaded", app = %app, version = %manifest.version, bytes = set.total_bytes());
        Ok(Prepared {
            app: app.clone(),
            version: manifest.version.clone(),
            payload: Box::new(set),
        })
    }

    fn activate(&self, prepared: Prepared) {
        let set = *prepared
            .payload
            .downcast::<StaticSet>()
            .expect("prepared by StaticRuntime");
        let fallback = match self.live.get(&prepared.app).as_deref() {
            Some(AppHandle::Static(old)) => Some(Fallback {
                set: Arc::new(old.current.immutable_only()),
                until: Instant::now() + self.grace,
            }),
            _ => None,
        };
        self.live.swap(
            &prepared.app,
            AppHandle::Static(StaticApp {
                current: Arc::new(set),
                fallback,
            }),
        );
    }
}

/// PHP releases until the worker pool exists: prepare checks the release is on disk; activate puts the placeholder

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use speedyspec_server_domain::{AppKind, ReleaseManifest, StaticSection};

    use super::*;

    fn release(dir: &Path, version: &str, files: &[(&str, &str)]) -> ReleaseManifest {
        for (rel, body) in files {
            let path = dir.join("dist").join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        ReleaseManifest {
            app: AppId::parse("admin").unwrap(),
            version: semver::Version::parse(version).unwrap(),
            kind: AppKind::Static,
            requires: Default::default(),
            static_: Some(StaticSection {
                root: "dist".to_string(),
                maintenance_page: None,
                not_found: None,
                cache: vec![speedyspec_server_domain::CacheRule {
                    pattern: "_astro/**".to_string(),
                    cache_control: "public, max-age=31536000, immutable".to_string(),
                }],
            }),
            php: None,
            protocol: None,
            contracts: Vec::new(),
        }
    }

    fn live(app: &AppId) -> Arc<LiveApps> {
        Arc::new(LiveApps::new(
            [(app.clone(), AppHandle::NotInstalled)]
                .into_iter()
                .collect::<HashMap<_, _>>(),
        ))
    }

    fn handle(live: &LiveApps, app: &AppId) -> StaticApp {
        match &*live.get(app).unwrap() {
            AppHandle::Static(app) => app.clone(),
            other => panic!("expected a static handle, got {other:?}"),
        }
    }

    #[test]
    fn first_activate_has_no_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppId::parse("admin").unwrap();
        let live = live(&app);
        let runtime = StaticRuntime::new(live.clone(), u64::MAX, Duration::from_secs(60));
        let manifest = release(dir.path(), "1.0.0", &[("index.html", "one")]);
        runtime.activate(runtime.prepare(&app, &manifest, dir.path()).unwrap());
        let current = handle(&live, &app);
        assert!(current.fallback.is_none());
        assert!(current.current.file("index.html").is_some());
    }

    #[test]
    fn static_activate_sets_fallback_from_old_set() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let app = AppId::parse("admin").unwrap();
        let live = live(&app);
        let runtime = StaticRuntime::new(live.clone(), u64::MAX, Duration::from_secs(60));
        let one = release(
            a.path(),
            "1.0.0",
            &[("index.html", "one"), ("_astro/one.js", "1")],
        );
        let two = release(
            b.path(),
            "2.0.0",
            &[("index.html", "two"), ("_astro/two.js", "2")],
        );
        runtime.activate(runtime.prepare(&app, &one, a.path()).unwrap());
        let before = Instant::now();
        runtime.activate(runtime.prepare(&app, &two, b.path()).unwrap());
        let current = handle(&live, &app);
        let fallback = current.fallback.unwrap();
        assert!(fallback.set.file("_astro/one.js").is_some());
        assert!(fallback.set.file("index.html").is_none());
        assert!(fallback.until >= before + Duration::from_secs(59));
        assert!(current.current.file("_astro/two.js").is_some());
    }

    #[test]
    fn static_prepare_fails_over_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppId::parse("admin").unwrap();
        let runtime = StaticRuntime::new(live(&app), 4, Duration::from_secs(60));
        let manifest = release(
            dir.path(),
            "1.0.0",
            &[("index.html", "more than four bytes")],
        );
        let err = runtime.prepare(&app, &manifest, dir.path()).err().unwrap();
        assert!(err.contains("limit"), "{err}");
    }
}
