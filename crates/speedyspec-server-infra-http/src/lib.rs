//! The release source over HTTPS: the release index, and downloads that resume.

use std::fs::OpenOptions;
use std::path::Path;
use std::time::Duration;

use speedyspec_server_domain::{ReleaseIndex, ReleaseSource, SourceError};
use ureq::Agent;

/// Fetches indexes and artifacts over HTTPS (or plain HTTP from a loopback host, for tests and local mirrors).
pub struct HttpSource {
    agent: Agent,
}

fn error(url: &str, message: impl ToString) -> SourceError {
    SourceError {
        url: url.to_string(),
        message: message.to_string(),
    }
}

impl Default for HttpSource {
    fn default() -> Self {
        HttpSource::new()
    }
}

impl HttpSource {
    pub fn new() -> HttpSource {
        let agent = Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_per_read(Some(Duration::from_secs(60)))
            // A source that trickles bytes must not hold an install open for ever.
            .timeout_global(Some(Duration::from_secs(30 * 60)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        HttpSource { agent }
    }

    /// Refuses anything but `https://`, and `http://` with a loopback host.
    fn allowed(url: &str) -> Result<(), SourceError> {
        let parsed = url::Url::parse(url).map_err(|err| error(url, err))?;
        let loopback = match parsed.host() {
            Some(url::Host::Domain(name)) => name == "localhost",
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        };
        match parsed.scheme() {
            "https" => Ok(()),
            "http" if loopback => Ok(()),
            _ => Err(error(
                url,
                "release URLs must use https (plain http only from a loopback host)",
            )),
        }
    }
}

impl ReleaseSource for HttpSource {
    fn index(&self, url: &str) -> Result<ReleaseIndex, SourceError> {
        HttpSource::allowed(url)?;
        let mut response = self.agent.get(url).call().map_err(|err| error(url, err))?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(error(url, format!("HTTP {status}")));
        }
        let bytes = response
            .body_mut()
            .read_to_vec()
            .map_err(|err| error(url, err))?;
        ReleaseIndex::parse(&bytes).map_err(|err| error(url, err))
    }

    fn download(&self, url: &str, dest: &Path) -> Result<(), SourceError> {
        HttpSource::allowed(url)?;
        let have = std::fs::metadata(dest).map(|meta| meta.len()).unwrap_or(0);
        let mut request = self.agent.get(url);
        if have > 0 {
            request = request.header("Range", &format!("bytes={have}-"));
        }
        let response = request.call().map_err(|err| error(url, err))?;
        let status = response.status().as_u16();
        let append = match status {
            206 if have > 0 => true,
            200 => false,
            416 if have > 0 => return Ok(()),
            other => return Err(error(url, format!("HTTP {other}"))),
        };
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(dest)
            .map_err(|err| error(url, format!("opening {}: {err}", dest.display())))?;
        let mut body = response.into_body().into_reader();
        std::io::copy(&mut body, &mut file)
            .map_err(|err| error(url, format!("download interrupted: {err}")))?;

        Ok(())
    }
}
