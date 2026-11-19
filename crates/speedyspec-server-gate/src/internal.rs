//! Paths only the server may call: a visitor's request for one answers 404 and never reaches the app.

/// Whether `rel_path` (normalised, relative to the mount, without its leading `/`) reaches one of the app's
/// `internal` paths (each written with its leading `/`). It does when it is that path, or below it (a script's
/// `PATH_INFO`), or when what follows any `.php` segment is (a front controller routing on `PATH_INFO`). A trailing
/// `/` does not make a path different.
pub fn is_internal(rel_path: &str, internal: &[String]) -> bool {
    let wanted = rel_path.trim_end_matches('/');
    if wanted.is_empty() {
        return false;
    }
    // The path itself, and what follows each `.php` segment along it.
    let segments: Vec<&str> = wanted.split('/').collect();
    let mut candidates = vec![wanted.to_string()];
    for (i, segment) in segments.iter().enumerate() {
        if segment.to_ascii_lowercase().ends_with(".php") && i + 1 < segments.len() {
            candidates.push(segments[i + 1..].join("/"));
        }
    }
    internal.iter().any(|path| {
        let path = path.trim_start_matches('/').trim_end_matches('/');
        !path.is_empty()
            && candidates.iter().any(|candidate| {
                candidate == path
                    || candidate
                        .strip_prefix(path)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
    })
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

    #[test]
    fn path_info_suffixes_and_front_controller_paths_are_internal_too() {
        let scripts = vec!["/gate/changes.php".to_string(), "/migrate.php".to_string()];
        for path in [
            "gate/changes.php/x",
            "migrate.php/anything/at/all",
            "index.php/gate/changes.php",
            "a/b.php/migrate.php",
        ] {
            assert!(is_internal(path, &scripts), "{path}");
        }
        let routes = vec!["/v1/gate/changes".to_string()];
        for path in [
            "index.php/v1/gate/changes",
            "index.php/v1/gate/changes/x",
            "v1/gate/changes/extra",
        ] {
            assert!(is_internal(path, &routes), "{path}");
        }
        for path in [
            "gate/changes.phpx",
            "migrate.phps",
            "index.php/v1/gate/changesx",
            "index.php",
        ] {
            assert!(
                !is_internal(path, &scripts) && !is_internal(path, &routes),
                "{path}"
            );
        }
    }
}
