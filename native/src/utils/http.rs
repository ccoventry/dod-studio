//! The one HTTP GET the app makes (HD tool downloads, map fetches), so both
//! report a failed request or a non-2xx reply in the same words (#485).

/// GETs `url` and hands back the response once its status is a success. The
/// error names the URL and either the transport failure or the status.
pub fn get(url: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| crate::messages::labeled(url, e))?;
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
