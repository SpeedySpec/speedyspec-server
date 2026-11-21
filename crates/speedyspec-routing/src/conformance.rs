//! Checks any `RouteTableStore` implementation must pass.

use speedyspec_http::Method;

use crate::{Fingerprint, RouteData, RouteId, RouteTableStore, Rule, StaticRoute};

fn data(path: &str) -> RouteData {
    RouteData {
        statics: vec![StaticRoute {
            method: Method::Get,
            path: path.to_string(),
            id: RouteId(0),
        }],
        dynamics: vec![],
        before_rules: vec![],
        after_rules: vec![Rule {
            pattern: "x/?$".into(),
            query: "index.php?x=1".into(),
        }],
        names: vec![],
    }
}

/// Runs every check, naming the first that fails.
pub fn check(store: &dyn RouteTableStore) -> Result<(), String> {
    let a = Fingerprint([1; 32]);
    let b = Fingerprint([2; 32]);
    let err = |e| format!("store error: {e}");
    if store.get(&a).map_err(err)?.is_some() {
        return Err("an empty store must miss".into());
    }
    store.put(&a, &data("/a")).map_err(err)?;
    if store.get(&a).map_err(err)? != Some(data("/a")) {
        return Err("a stored entry must come back equal".into());
    }
    store.put(&a, &data("/a2")).map_err(err)?;
    if store.get(&a).map_err(err)? != Some(data("/a2")) {
        return Err("put must overwrite".into());
    }
    if store.get(&b).map_err(err)?.is_some() {
        return Err("fingerprints must be independent".into());
    }
    store.put(&b, &data("/b")).map_err(err)?;
    store.forget(&a).map_err(err)?;
    if store.get(&a).map_err(err)?.is_some() {
        return Err("forget must remove the entry".into());
    }
    if store.get(&b).map_err(err)? != Some(data("/b")) {
        return Err("forget must leave other entries".into());
    }
    store.forget(&b).map_err(err)?;
    Ok(())
}
