//! The one HTTP GET the app makes (HD tool downloads, map fetches), so both
//! report a failed request or a non-2xx reply in the same words (#485).

/// GETs `url` and hands back the response once its status is a success. The
/// error names the URL and either the transport failure or the status.
pub fn get(url: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
    check(url, ureq::get(url).call())
}

/// Like [`get`], but gives up after `timeout` in total: ureq has no time limit
/// of its own, so a stalled connection would otherwise wait forever. For reads
/// a window waits on (the What's new release list, #1013).
pub fn get_with_timeout(
    url: &str,
    timeout: std::time::Duration,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into();
    check(url, agent.get(url).call())
}

fn check(
    url: &str,
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    let response = result.map_err(|e| crate::messages::labeled(url, e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(crate::messages::url_returned_status(url, status));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A request that never reaches a server fails with the URL in the text,
    /// without needing the network.
    #[test]
    fn a_failed_request_names_the_url() {
        let url = "http://127.0.0.1:1/nothing-listens-here";
        let err = get(url).unwrap_err();
        assert!(err.contains(url), "{err}");
    }
}
