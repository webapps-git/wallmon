//! Websites: a real GET to each, all at once, with a time limit. Redirects are not followed,
//! so a 301 is reported as such and judged by the accepted codes.

use std::time::{Duration, Instant};

use crate::config;
use crate::state::{Site, SiteOutcome};

pub const LIMIT: Duration = Duration::from_secs(10);

pub struct Reader {
    sites: Vec<config::Site>,
    agent: ureq::Agent,
}

impl Reader {
    pub fn new(sites: Vec<config::Site>) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(LIMIT))
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(concat!("wallmon/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Reader { sites, agent }
    }

    pub fn read(&mut self) -> Result<Vec<Site>, String> {
        let agent = &self.agent;
        Ok(std::thread::scope(|scope| {
            let checks: Vec<_> = self.sites.iter().map(|s| scope.spawn(move || check(agent, s))).collect();
            checks
                .into_iter()
                .zip(&self.sites)
                .map(|(h, s)| {
                    h.join().unwrap_or_else(|_| Site {
                        name: s.name().to_string(),
                        outcome: SiteOutcome::Error("error".into()),
                        accepted: false,
                    })
                })
                .collect()
        }))
    }
}

fn check(agent: &ureq::Agent, site: &config::Site) -> Site {
    let start = Instant::now();
    let outcome = match agent.get(&site.url).call() {
        // Only the headers are needed; the body is dropped unread.
        Ok(res) => SiteOutcome::Response { code: res.status().as_u16(), time: start.elapsed() },
        Err(ureq::Error::Timeout(_)) => SiteOutcome::Timeout,
        Err(e) => SiteOutcome::Error(short_error(&e).into()),
    };
    let accepted = matches!(outcome, SiteOutcome::Response { code, .. } if site.accepts(code));
    Site { name: site.name().to_string(), outcome, accepted }
}

/// Fits the time column of the panel. DNS and certificate failures usually arrive wrapped
/// in I/O errors, so the message text decides.
fn short_error(e: &ureq::Error) -> &'static str {
    let text = e.to_string().to_lowercase();
    match e {
        ureq::Error::HostNotFound => "DNS error",
        ureq::Error::Rustls(_) | ureq::Error::Tls(_) => "TLS error",
        _ if text.contains("certificate") || text.contains("tls") => "TLS error",
        _ if text.contains("lookup") || text.contains("resolve") || text.contains("name or service") => "DNS error",
        _ if text.contains("refused") => "refused",
        _ if text.contains("timed out") => "timeout",
        ureq::Error::ConnectionFailed => "no connection",
        _ => "error",
    }
}
