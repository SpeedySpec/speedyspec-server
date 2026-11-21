use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

use serde_json::{Value, json};

use crate::wordpress_release;

/// What one oracle call answered: the return value (strings as `{"b64"}`, arrays as `{"array"}`) and how many
/// times each filter fired, in first-fired order.
#[derive(Clone, Debug, PartialEq)]
pub struct OracleResult {
    pub returned: Value,
    pub filters: Vec<(String, u32)>,
}

/// A long-lived PHP process running the pinned release's kses.php.
///
/// The PHP binary is `SPEEDYSPEC_PHP` or `php` on `PATH`; a missing PHP fails the test rather than skipping it,
/// because these comparisons are the port's proof.
pub struct PhpOracle {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    timeout: Duration,
}

/// How long one call may take before the oracle counts PHP as hung. `SPEEDYSPEC_ORACLE_TIMEOUT` (seconds) changes it.
fn call_timeout() -> Duration {
    let seconds = std::env::var("SPEEDYSPEC_ORACLE_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    Duration::from_secs(seconds)
}

/// The error a call returns when PHP did not answer in time; the oracle restarts PHP before returning it.
pub const PHP_TIMED_OUT: &str = "PHP timed out";

impl PhpOracle {
    pub fn start() -> Result<PhpOracle, String> {
        let root = wordpress_release()?;
        let php = std::env::var("SPEEDYSPEC_PHP").unwrap_or_else(|_| "php".to_string());
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("php/oracle.php");
        let mut child = Command::new(&php)
            .args(["-d", "pcre.jit=1", "-d", "memory_limit=2G"])
            .arg(&script)
            .arg(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{php}: {e} (set SPEEDYSPEC_PHP to a PHP 8.4+ CLI)"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let mut stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            loop {
                let mut line = String::new();
                match stdout.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Ok(PhpOracle {
            child,
            stdin,
            lines,
            timeout: call_timeout(),
        })
    }

    /// Sets how long one call may take before PHP counts as hung (see [`PHP_TIMED_OUT`]).
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Calls a kses.php function with arguments already encoded (see [`crate::bytes`]).
    pub fn call(&mut self, function: &str, args: Vec<Value>) -> Result<OracleResult, String> {
        self.request(json!({ "fn": function, "args": args }))
    }

    /// Calls a function as if inside the filter `current_filter` (for `wp_filter_kses` and friends).
    pub fn call_in_filter(
        &mut self,
        filter: &str,
        function: &str,
        args: Vec<Value>,
    ) -> Result<OracleResult, String> {
        self.request(json!({ "fn": function, "args": args, "current_filter": filter }))
    }

    /// The four allowlist globals after kses.php's own setup.
    pub fn globals(&mut self) -> Result<Value, String> {
        Ok(self.call("globals", vec![])?.returned)
    }

    fn request(&mut self, request: Value) -> Result<OracleResult, String> {
        writeln!(self.stdin, "{request}").map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())?;
        let line = match self.lines.recv_timeout(self.timeout) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => {
                let _ = self.child.kill();
                let _ = self.child.wait();
                let timeout = self.timeout;
                *self = PhpOracle::start()?;
                self.timeout = timeout;
                return Err(PHP_TIMED_OUT.to_string());
            }
            Err(RecvTimeoutError::Disconnected) => return Err("the PHP oracle exited".to_string()),
        };
        let response: Value = serde_json::from_str(&line).map_err(|e| format!("{e}: {line}"))?;
        if let Some(error) = response.get("error") {
            return Err(format!("PHP: {error}"));
        }
        let filters = response["filters"]
            .as_array()
            .map(|all| {
                all.iter()
                    .map(|p| {
                        (
                            p[0].as_str().unwrap_or_default().to_string(),
                            p[1].as_u64().unwrap_or(0) as u32,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(OracleResult {
            returned: response["returned"].clone(),
            filters,
        })
    }
}

impl Drop for PhpOracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
