//! Paths only the server may call: a visitor's request for one answers 404 and never reaches the app.

/// Whether `rel_path` (normalised, relative to the mount, without its leading `/`) is one of the app's `internal`
/// paths (each written with its leading `/`). A trailing `/` does not make a path different.
pub fn is_internal(rel_path: &str, internal: &[String]) -> bool {
    let wanted = rel_path.trim_end_matches('/');
    !wanted.is_empty()
        && internal
            .iter()
            .any(|path| path.trim_start_matches('/').trim_end_matches('/') == wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_paths_match_after_normalisation() {
        let internal = vec!["/v1/gate/changes".to_string()];
        for path in ["v1/gate/changes", "v1/gate/changes/"] {
            assert!(is_internal(path, &internal), "{path}");
        }
        assert!(!is_internal("v1/gate/changesx", &internal));
        assert!(!is_internal("v1/gate", &internal));
        assert!(!is_internal("", &internal));
    }
}
