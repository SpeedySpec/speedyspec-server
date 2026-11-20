//! `PhpWpdbHost`: the host port over PHP's WordPress functions, with WordPress's own fallbacks when one is absent.

use speedyspec_php::PhpError;
use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_db::{DoingItWrong, WpdbHost};

fn call(name: &str, args: &[Value]) -> Result<Value, PhpError> {
    speedyspec_php::call(&Value::Str(name.into()), args)
}

fn exists(function: &str) -> bool {
    matches!(
        call("function_exists", &[Value::Str(function.into())]),
        Ok(Value::Bool(true))
    )
}

fn text(v: Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s),
        Value::Bytes(b) => Some(String::from_utf8_lossy(&b).into_owned()),
        _ => None,
    }
}

/// What only PHP's WordPress can do for `wpdb`.
pub struct PhpWpdbHost;

impl WpdbHost for PhpWpdbHost {
    fn doing_it_wrong(&self, notice: &DoingItWrong) {
        if exists("_doing_it_wrong") {
            let _ = call(
                "_doing_it_wrong",
                &[
                    Value::Str(notice.function.clone()),
                    Value::Str(notice.message.clone()),
                    Value::Str(notice.version.clone()),
                ],
            );
        } else {
            let _ = call(
                "trigger_error",
                &[
                    Value::Str(format!("{}: {}", notice.function, notice.message)),
                    Value::Int(1024),
                ],
            );
        }
    }

    fn translate(&self, text_in: &str) -> String {
        if exists("wp_load_translations_early") {
            let _ = call("wp_load_translations_early", &[]);
        }
        if exists("__")
            && let Ok(v) = call("__", &[Value::Str(text_in.into())])
        {
            return text(v).unwrap_or_else(|| text_in.to_string());
        }
        text_in.to_string()
    }

    fn constant(&self, name: &str) -> Option<Value> {
        match call("defined", &[Value::Str(name.into())]) {
            Ok(Value::Bool(true)) => call("constant", &[Value::Str(name.into())]).ok(),
            _ => None,
        }
    }

    fn is_multisite(&self) -> bool {
        exists("is_multisite") && matches!(call("is_multisite", &[]), Ok(Value::Bool(true)))
    }

    fn error_log(&self, message: &str, destination: Option<&str>) {
        let _ = match destination {
            Some(file) => call(
                "error_log",
                &[
                    Value::Str(message.into()),
                    Value::Int(3),
                    Value::Str(file.into()),
                ],
            ),
            None => call("error_log", &[Value::Str(message.into())]),
        };
    }

    fn output(&self, html: &str) {
        let _ = call(
            "printf",
            &[Value::Str("%s".into()), Value::Str(html.into())],
        );
    }

    fn die(&self, message: &str) {
        if exists("wp_die") {
            let _ = call("wp_die", &[Value::Str(message.into())]);
        } else {
            self.output(message);
            let _ = speedyspec_php::eval("speedyspec://wp-db-die", "die();");
        }
    }

    fn dead_db(&self) {
        if exists("dead_db") {
            let _ = call("dead_db", &[]);
        }
    }

    fn record_error(&self, query: &str, error: &str) {
        let entry = Value::Array(vec![
            (Key::Str("query".into()), Value::Str(query.into())),
            (Key::Str("error_str".into()), Value::Str(error.into())),
        ]);
        if !matches!(speedyspec_php::global_push("EZSQL_ERROR", &entry), Ok(true)) {
            let _ = speedyspec_php::set_global(
                "EZSQL_ERROR",
                &Value::Array(vec![(Key::Int(0), entry)]),
            );
        }
    }

    fn caller(&self) -> String {
        if exists("wp_debug_backtrace_summary")
            && let Ok(v) = call("wp_debug_backtrace_summary", &[Value::Str("wpdb".into())])
        {
            return text(v).unwrap_or_default();
        }
        String::new()
    }

    fn deprecated_function(&self, function: &str, version: &str, replacement: &str) {
        if exists("_deprecated_function") {
            let _ = call(
                "_deprecated_function",
                &[
                    Value::Str(function.into()),
                    Value::Str(version.into()),
                    Value::Str(replacement.into()),
                ],
            );
        }
    }
}
