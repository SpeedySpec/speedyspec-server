use regex::RegexBuilder;
use speedyspec_http_message::HttpServerRequest;
use speedyspec_router::WpSubject;

/// `WP::parse_request`'s requested path and file, from the request's server params.
///
/// `home_path` is the path of `home_url()` (empty at the root). `None` unless the server sent the request to the
/// app's entry script (`index`), since only `index.php` runs `parse_request`.
pub fn wp_subject(request: &HttpServerRequest, home_path: &str, index: &str) -> Option<WpSubject> {
    let script = request.server_param("SCRIPT_NAME").unwrap_or("");
    if !script.ends_with(&format!("/{index}")) && script != index {
        return None;
    }
    let pathinfo = request.server_param("PATH_INFO").unwrap_or("");
    let pathinfo = pathinfo.split('?').next().unwrap_or("").replace('%', "%25");
    let req_uri = request.server_param("REQUEST_URI").unwrap_or("");
    let req_uri = req_uri.split('?').next().unwrap_or("");

    let home_path = home_path.trim_matches('/');
    let strip_home = |value: &str| -> String {
        if home_path.is_empty() {
            return value.to_string();
        }
        let pattern = format!("^{}", regex::escape(home_path));
        match RegexBuilder::new(&pattern).case_insensitive(true).build() {
            Ok(re) => re.replace(value, "").trim_matches('/').to_string(),
            Err(_) => value.to_string(),
        }
    };

    let req_uri = if pathinfo.is_empty() {
        req_uri.to_string()
    } else {
        req_uri.replace(&pathinfo, "")
    };
    let req_uri = strip_home(req_uri.trim_matches('/'));
    let pathinfo = strip_home(pathinfo.trim_matches('/'));

    let index_pattern = format!("^.*{index}$");
    let pathinfo_is_index =
        regex::Regex::new(&index_pattern).is_ok_and(|re| re.is_match(&pathinfo));
    let (requested_path, req_uri) = if !pathinfo.is_empty() && !pathinfo_is_index {
        (pathinfo, req_uri)
    } else {
        let req_uri = if req_uri == index {
            String::new()
        } else {
            req_uri
        };
        (req_uri.clone(), req_uri)
    };
    Some(WpSubject {
        requested_path,
        requested_file: req_uri,
    })
}
