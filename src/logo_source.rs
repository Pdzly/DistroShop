use base64::{encoded_len, engine::general_purpose::STANDARD, Engine as _};
use reqwest::header::{HeaderValue, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use serde::{Deserialize, Serialize};
use std::{
    error::Error as _,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration as StdDuration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Default)]
pub(crate) struct CachedLogo {
    pub source: Option<Arc<str>>,
    pub warning: Option<String>,
    pub used_alternate: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct CacheEntry {
    /// URL that supplied the currently cached bytes.
    url: String,
    /// Preferred URL from the catalog when those bytes were fetched.
    ///
    /// Entries written before fallback support do not have this field.
    #[serde(default)]
    requested_primary_url: Option<String>,
    data_url: String,
    etag: Option<String>,
    last_modified: Option<String>,
}

fn cache_path(cache_dir: &Path, id: u8) -> PathBuf {
    cache_dir.join(format!("{id}.json"))
}

fn data_url(bytes: &[u8], mime_type: &str) -> String {
    let prefix = "data:".len() + mime_type.len() + ";base64,".len();
    let payload = encoded_len(bytes.len(), true).expect("logo is too large to encode");
    let mut source = String::with_capacity(prefix + payload);
    source.push_str("data:");
    source.push_str(mime_type);
    source.push_str(";base64,");
    STANDARD.encode_string(bytes, &mut source);
    source
}

fn image_mime_type(bytes: &[u8]) -> Option<&'static str> {
    if is_svg(bytes) {
        return Some("image/svg+xml");
    }

    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => Some("image/png"),
        image::ImageFormat::Jpeg => Some("image/jpeg"),
        image::ImageFormat::Gif => Some("image/gif"),
        image::ImageFormat::WebP => Some("image/webp"),
        image::ImageFormat::Bmp => Some("image/bmp"),
        image::ImageFormat::Ico => Some("image/x-icon"),
        image::ImageFormat::Avif => Some("image/avif"),
        _ => None,
    }
}

fn is_svg(bytes: &[u8]) -> bool {
    let Ok(mut remaining) = std::str::from_utf8(bytes) else {
        return false;
    };
    remaining = remaining.trim_start_matches('\u{feff}');

    loop {
        remaining = remaining.trim_start();
        if let Some(after_start) = remaining.strip_prefix("<?xml") {
            let Some(end) = after_start.find("?>") else {
                return false;
            };
            remaining = &after_start[end + 2..];
        } else if let Some(after_start) = remaining.strip_prefix("<!--") {
            let Some(end) = after_start.find("-->") else {
                return false;
            };
            remaining = &after_start[end + 3..];
        } else if let Some(after_start) = remaining.strip_prefix("<!DOCTYPE") {
            let Some(end) = doctype_end(after_start) else {
                return false;
            };
            remaining = &after_start[end + 1..];
        } else {
            break;
        }
    }

    let Some(after_svg) = remaining.strip_prefix("<svg") else {
        return false;
    };
    matches!(
        after_svg.chars().next(),
        Some('>' | '/' | ' ' | '\t' | '\r' | '\n')
    )
}

fn doctype_end(text: &str) -> Option<usize> {
    let mut brackets = 0_u32;
    let mut quote = None;
    for (index, character) in text.char_indices() {
        if let Some(open_quote) = quote {
            if character == open_quote {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '[' => brackets += 1,
            ']' => brackets = brackets.checked_sub(1)?,
            '>' if brackets == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn valid_data_url(data_url: &str) -> bool {
    let Some((prefix, encoded)) = data_url.split_once(",") else {
        return false;
    };
    let Some(mime_type) = prefix
        .strip_prefix("data:")
        .and_then(|prefix| prefix.strip_suffix(";base64"))
    else {
        return false;
    };
    let Ok(bytes) = STANDARD.decode(encoded) else {
        return false;
    };
    image_mime_type(&bytes) == Some(mime_type)
}

fn read_cache(path: &Path) -> Option<CacheEntry> {
    let content = fs::read(path).ok()?;
    let entry: CacheEntry = serde_json::from_slice(&content).ok()?;
    valid_data_url(&entry.data_url).then_some(entry)
}

fn write_cache(path: &Path, entry: &CacheEntry) -> std::io::Result<()> {
    let parent = path.parent().expect("cache path always has a parent");
    fs::create_dir_all(parent)?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), nonce));
    let result = (|| {
        let serialized = serde_json::to_vec(entry).map_err(std::io::Error::other)?;
        fs::write(&temporary, serialized)?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn header_value(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_owned)
}

fn cached_logo(entry: CacheEntry, warning: Option<String>, used_alternate: bool) -> CachedLogo {
    CachedLogo {
        source: Some(Arc::from(entry.data_url)),
        warning,
        used_alternate,
    }
}

fn warning_with_stale_cache(message: String, stale: Option<CacheEntry>) -> CachedLogo {
    match stale {
        Some(entry) => cached_logo(entry, Some(message), false),
        None => CachedLogo {
            source: None,
            warning: Some(message),
            used_alternate: false,
        },
    }
}

fn error_chain(error: &reqwest::Error) -> String {
    use std::fmt::Write as _;

    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        write!(&mut message, ": {cause}").expect("writing to a String cannot fail");
        source = cause.source();
    }
    message
}

fn retryable_request_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_body()
}

fn cache_matches_request(entry: &CacheEntry, primary_url: &str, candidates: &[&str]) -> bool {
    let requested_primary_matches = entry
        .requested_primary_url
        .as_deref()
        .map_or(entry.url == primary_url, |stored| stored == primary_url);
    requested_primary_matches && candidates.iter().any(|candidate| *candidate == entry.url)
}

enum FetchResult {
    Modified(CacheEntry),
    NotModified {
        etag: Option<String>,
        last_modified: Option<String>,
    },
}

async fn fetch_candidate(
    client: &reqwest::Client,
    url: &str,
    cached: Option<&CacheEntry>,
) -> Result<FetchResult, String> {
    const RETRY_DELAY: StdDuration = StdDuration::from_millis(75);
    let use_validators = cached.is_some_and(|entry| entry.url == url);

    for attempt in 0..2 {
        let mut request = client.get(url);
        if use_validators {
            if let Some(etag) = cached.and_then(|entry| entry.etag.as_deref()) {
                if let Ok(value) = HeaderValue::from_str(etag) {
                    request = request.header(IF_NONE_MATCH, value);
                }
            }
            if let Some(last_modified) = cached.and_then(|entry| entry.last_modified.as_deref()) {
                if let Ok(value) = HeaderValue::from_str(last_modified) {
                    request = request.header(IF_MODIFIED_SINCE, value);
                }
            }
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                let failure = format!("network error: {}", error_chain(&error));
                if attempt == 0 && retryable_request_error(&error) {
                    tokio::time::sleep(RETRY_DELAY).await;
                    continue;
                }
                return Err(failure);
            }
        };
        let status = response.status();

        if status == reqwest::StatusCode::NOT_MODIFIED {
            return use_validators
                .then(|| FetchResult::NotModified {
                    etag: header_value(response.headers(), ETAG),
                    last_modified: header_value(response.headers(), LAST_MODIFIED),
                })
                .ok_or_else(|| "server returned 304 without a matching cache entry".to_owned());
        }
        if !status.is_success() {
            let failure = format!("server returned {status}");
            if attempt == 0
                && (status == reqwest::StatusCode::REQUEST_TIMEOUT
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || status.is_server_error())
            {
                tokio::time::sleep(RETRY_DELAY).await;
                continue;
            }
            return Err(failure);
        }

        let etag = header_value(response.headers(), ETAG);
        let last_modified = header_value(response.headers(), LAST_MODIFIED);
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                let failure = format!("could not read response: {}", error_chain(&error));
                if attempt == 0 && retryable_request_error(&error) {
                    tokio::time::sleep(RETRY_DELAY).await;
                    continue;
                }
                return Err(failure);
            }
        };
        let Some(mime_type) = image_mime_type(&bytes) else {
            return Err("response was not an image".to_owned());
        };

        return Ok(FetchResult::Modified(CacheEntry {
            url: url.to_owned(),
            requested_primary_url: None,
            data_url: data_url(&bytes, mime_type),
            etag,
            last_modified,
        }));
    }

    unreachable!("each logo request attempt returns or retries once")
}

/// Loads a distro logo from the persistent cache or downloads it when needed.
///
/// A non-refresh call never makes a request when its requested primary URL and
/// permitted sources still identify a valid cached image. Refreshes apply
/// validators only to the URL that supplied the cached bytes.
pub(crate) async fn load_logo(
    client: &reqwest::Client,
    cache_dir: &Path,
    id: u8,
    url: &str,
    fallbacks: &[String],
    refresh: bool,
) -> CachedLogo {
    let path = cache_path(cache_dir, id);
    let mut cached = read_cache(&path);
    let mut candidates = Vec::with_capacity(fallbacks.len() + 1);
    for candidate in std::iter::once(url).chain(fallbacks.iter().map(String::as_str)) {
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }

    if !refresh
        && cached
            .as_ref()
            .is_some_and(|entry| cache_matches_request(entry, url, &candidates))
    {
        let entry = cached.expect("matching cache entry remains available");
        let used_alternate = entry.url != url;
        return cached_logo(entry, None, used_alternate);
    }

    let mut failures = Vec::new();
    for candidate in candidates {
        let result = fetch_candidate(
            client,
            candidate,
            cached.as_ref().filter(|entry| entry.url == candidate),
        )
        .await;
        let used_alternate = candidate != url;

        match result {
            Ok(FetchResult::NotModified {
                etag,
                last_modified,
            }) => {
                let mut entry = cached
                    .take()
                    .expect("304 requires the matching cache entry");
                let mut metadata_changed = false;
                if entry.requested_primary_url.as_deref() != Some(url) {
                    entry.requested_primary_url = Some(url.to_owned());
                    metadata_changed = true;
                }
                if let Some(etag) = etag {
                    if entry.etag.as_deref() != Some(etag.as_str()) {
                        entry.etag = Some(etag);
                        metadata_changed = true;
                    }
                }
                if let Some(last_modified) = last_modified {
                    if entry.last_modified.as_deref() != Some(last_modified.as_str()) {
                        entry.last_modified = Some(last_modified);
                        metadata_changed = true;
                    }
                }
                if metadata_changed {
                    if let Err(error) = write_cache(&path, &entry) {
                        return cached_logo(
                            entry,
                            Some(format!("Could not save distro logo cache: {error}")),
                            used_alternate,
                        );
                    }
                }
                return cached_logo(entry, None, used_alternate);
            }
            Ok(FetchResult::Modified(mut entry)) => {
                entry.requested_primary_url = Some(url.to_owned());
                let unchanged = cached.as_ref().is_some_and(|previous| {
                    previous.url == entry.url
                        && previous.requested_primary_url == entry.requested_primary_url
                        && previous.data_url == entry.data_url
                        && previous.etag == entry.etag
                        && previous.last_modified == entry.last_modified
                });
                if unchanged {
                    return cached_logo(
                        cached.expect("unchanged cache entry remains available"),
                        None,
                        used_alternate,
                    );
                }
                if let Err(error) = write_cache(&path, &entry) {
                    return cached_logo(
                        entry,
                        Some(format!("Could not save distro logo cache: {error}")),
                        used_alternate,
                    );
                }
                return cached_logo(entry, None, used_alternate);
            }
            Err(failure) => failures.push(failure),
        }
    }

    warning_with_stale_cache(
        format!("Could not download distro logo: {}", failures.join("; ")),
        cached,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::{Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    const IMAGE_ONE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="red"/></svg>"#;
    const IMAGE_TWO: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="blue"/></svg>"#;

    struct Response {
        status: &'static str,
        headers: Vec<(&'static str, &'static str)>,
        body: &'static [u8],
    }

    struct Server {
        base_url: String,
        requests: Arc<Mutex<Vec<String>>>,
        worker: thread::JoinHandle<()>,
    }

    impl Server {
        fn start(responses: Vec<Response>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
            listener
                .set_nonblocking(true)
                .expect("set fixture nonblocking");
            let base_url = format!("http://{}", listener.local_addr().expect("local address"));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let captured = Arc::clone(&requests);
            let worker = thread::spawn(move || {
                let mut responses = VecDeque::from(responses);
                while let Some(response) = responses.pop_front() {
                    let deadline = Instant::now() + Duration::from_secs(3);
                    let mut stream = loop {
                        match listener.accept() {
                            Ok((stream, _)) => break stream,
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(Instant::now() < deadline, "expected image request");
                                thread::sleep(Duration::from_millis(5));
                            }
                            Err(error) => panic!("accept fixture request: {error}"),
                        }
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .expect("set request timeout");
                    captured
                        .lock()
                        .expect("request lock")
                        .push(read_request(&mut stream));
                    write!(
                        stream,
                        "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                        response.status,
                        response.body.len()
                    )
                    .expect("write status");
                    for (name, value) in response.headers {
                        write!(stream, "{name}: {value}\r\n").expect("write header");
                    }
                    stream.write_all(b"\r\n").expect("write header end");
                    stream.write_all(response.body).expect("write body");
                }
            });
            Self {
                base_url,
                requests,
                worker,
            }
        }

        fn url(&self, path: &str) -> String {
            format!("{}/{}", self.base_url, path)
        }

        fn finish(self) -> Vec<String> {
            self.worker.join().expect("fixture server thread");
            Arc::try_unwrap(self.requests)
                .expect("only test holds requests")
                .into_inner()
                .expect("request lock")
        }
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut byte = [0; 1];
        while stream.read_exact(&mut byte).is_ok() {
            request.push(byte[0]);
            if request.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        String::from_utf8(request).expect("utf8 request")
    }

    fn cache_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("distroshop-logo-source-{label}-{nonce}"));
        fs::create_dir_all(&directory).expect("create test cache directory");
        directory
    }

    fn write_test_cache(
        directory: &Path,
        id: u8,
        actual_url: &str,
        requested_primary_url: Option<&str>,
        bytes: &[u8],
        etag: Option<&str>,
    ) {
        write_cache(
            &cache_path(directory, id),
            &CacheEntry {
                url: actual_url.to_owned(),
                requested_primary_url: requested_primary_url.map(str::to_owned),
                data_url: data_url(bytes, image_mime_type(bytes).expect("test image")),
                etag: etag.map(str::to_owned),
                last_modified: None,
            },
        )
        .expect("write test cache");
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(1))
            .build()
            .expect("build client")
    }

    fn refused_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused address");
        let address = listener.local_addr().expect("local address");
        drop(listener);
        format!("http://{address}/primary")
    }

    #[tokio::test]
    async fn connection_refusal_uses_valid_alternate_and_marks_it() {
        let server = Server::start(vec![Response {
            status: "200 OK",
            headers: vec![],
            body: IMAGE_TWO,
        }]);
        let directory = cache_dir("connection-fallback");
        let primary = refused_url();
        let alternate = server.url("alternate");

        let logo = load_logo(
            &client(),
            &directory,
            1,
            &primary,
            std::slice::from_ref(&alternate),
            false,
        )
        .await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_TWO, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_none());
        assert!(logo.used_alternate);
        let stored = read_cache(&cache_path(&directory, 1)).expect("fallback cache");
        assert_eq!(stored.url, alternate);
        assert_eq!(
            stored.requested_primary_url.as_deref(),
            Some(primary.as_str())
        );
        assert_eq!(server.finish().len(), 1);
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn cached_alternate_starts_without_a_request() {
        let server = Server::start(vec![]);
        let directory = cache_dir("cached-alternate");
        let primary = server.url("primary");
        let alternate = server.url("alternate");
        write_test_cache(
            &directory,
            2,
            &alternate,
            Some(&primary),
            IMAGE_ONE,
            Some("alternate-v1"),
        );

        let logo = load_logo(
            &client(),
            &directory,
            2,
            &primary,
            std::slice::from_ref(&alternate),
            false,
        )
        .await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_ONE, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_none());
        assert!(logo.used_alternate);
        assert!(
            server.finish().is_empty(),
            "cache reuse made an HTTP request"
        );
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn refresh_validates_cached_alternate_only_at_its_own_url() {
        let server = Server::start(vec![
            Response {
                status: "500 Server Error",
                headers: vec![],
                body: b"failure",
            },
            Response {
                status: "500 Server Error",
                headers: vec![],
                body: b"failure",
            },
            Response {
                status: "304 Not Modified",
                headers: vec![],
                body: b"",
            },
        ]);
        let directory = cache_dir("alternate-304");
        let primary = server.url("primary");
        let alternate = server.url("alternate");
        write_test_cache(
            &directory,
            3,
            &alternate,
            Some(&primary),
            IMAGE_ONE,
            Some("alternate-v1"),
        );

        let logo = load_logo(
            &client(),
            &directory,
            3,
            &primary,
            std::slice::from_ref(&alternate),
            true,
        )
        .await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_ONE, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_none());
        assert!(logo.used_alternate);
        let requests = server.finish();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].starts_with("GET /primary "));
        assert!(requests[1].starts_with("GET /primary "));
        assert!(!requests[0].contains("if-none-match:"));
        assert!(!requests[1].contains("if-none-match:"));
        assert!(requests[2].starts_with("GET /alternate "));
        assert!(requests[2].contains("if-none-match: alternate-v1\r\n"));
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn new_primary_replaces_cached_fallback_before_trying_it_again() {
        let server = Server::start(vec![Response {
            status: "200 OK",
            headers: vec![("ETag", "primary-v2")],
            body: IMAGE_TWO,
        }]);
        let directory = cache_dir("new-primary");
        let new_primary = server.url("primary");
        let old_primary = "http://old.invalid/primary";
        let alternate = server.url("alternate");
        write_test_cache(
            &directory,
            4,
            &alternate,
            Some(old_primary),
            IMAGE_ONE,
            Some("alternate-v1"),
        );

        let logo = load_logo(
            &client(),
            &directory,
            4,
            &new_primary,
            std::slice::from_ref(&alternate),
            false,
        )
        .await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_TWO, "image/svg+xml").as_str())
        );
        assert!(!logo.used_alternate);
        let stored = read_cache(&cache_path(&directory, 4)).expect("new primary cache");
        assert_eq!(stored.url, new_primary);
        assert_eq!(
            stored.requested_primary_url.as_deref(),
            Some(new_primary.as_str())
        );
        let requests = server.finish();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /primary "));
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn all_failed_sources_keep_last_known_good_image() {
        let server = Server::start(vec![
            Response {
                status: "200 OK",
                headers: vec![],
                body: b"not an image",
            },
            Response {
                status: "404 Not Found",
                headers: vec![],
                body: b"missing",
            },
        ]);
        let directory = cache_dir("all-fail");
        let primary = server.url("primary");
        let alternate = server.url("alternate");
        write_test_cache(
            &directory,
            5,
            "http://old.invalid/logo",
            Some("http://old.invalid/logo"),
            IMAGE_ONE,
            None,
        );

        let logo = load_logo(
            &client(),
            &directory,
            5,
            &primary,
            std::slice::from_ref(&alternate),
            true,
        )
        .await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_ONE, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_some());
        assert_eq!(
            read_cache(&cache_path(&directory, 5))
                .expect("old cache")
                .data_url,
            data_url(IMAGE_ONE, "image/svg+xml")
        );
        assert_eq!(server.finish().len(), 2);
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn transient_response_retries_once_before_succeeding() {
        let server = Server::start(vec![
            Response {
                status: "500 Server Error",
                headers: vec![],
                body: b"retry",
            },
            Response {
                status: "200 OK",
                headers: vec![],
                body: IMAGE_ONE,
            },
        ]);
        let directory = cache_dir("transient-retry");
        let primary = server.url("primary");

        let logo = load_logo(&client(), &directory, 6, &primary, &[], false).await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_ONE, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_none());
        assert_eq!(server.finish().len(), 2);
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn non_image_primary_uses_alternate_without_a_retry() {
        let server = Server::start(vec![
            Response {
                status: "200 OK",
                headers: vec![("Content-Type", "text/plain")],
                body: b"not an image",
            },
            Response {
                status: "200 OK",
                headers: vec![],
                body: IMAGE_TWO,
            },
        ]);
        let directory = cache_dir("non-image-fallback");
        let primary = server.url("primary");
        let alternate = server.url("alternate");

        let logo = load_logo(
            &client(),
            &directory,
            7,
            &primary,
            std::slice::from_ref(&alternate),
            false,
        )
        .await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_TWO, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_none());
        assert!(logo.used_alternate);
        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /primary "));
        assert!(requests[1].starts_with("GET /alternate "));
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn identical_refresh_updates_validators_without_replacing_image() {
        let server = Server::start(vec![Response {
            status: "200 OK",
            headers: vec![("ETag", "logo-v2")],
            body: IMAGE_ONE,
        }]);
        let directory = cache_dir("unchanged-image");
        let primary = server.url("primary");
        write_test_cache(
            &directory,
            8,
            &primary,
            Some(&primary),
            IMAGE_ONE,
            Some("logo-v1"),
        );

        let logo = load_logo(&client(), &directory, 8, &primary, &[], true).await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_ONE, "image/svg+xml").as_str())
        );
        assert_eq!(
            read_cache(&cache_path(&directory, 8))
                .expect("updated cache")
                .etag
                .as_deref(),
            Some("logo-v2")
        );
        server.finish();
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn legacy_cache_entry_without_requested_primary_remains_usable() {
        let server = Server::start(vec![]);
        let directory = cache_dir("legacy-cache");
        let primary = server.url("primary");
        let data = data_url(IMAGE_ONE, "image/svg+xml");
        let legacy = serde_json::json!({
            "url": primary,
            "data_url": data,
            "etag": "legacy-v1",
            "last_modified": null,
        });
        fs::write(
            cache_path(&directory, 9),
            serde_json::to_vec(&legacy).expect("serialize legacy cache"),
        )
        .expect("write legacy cache");

        let logo = load_logo(&client(), &directory, 9, &primary, &[], false).await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_ONE, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_none());
        assert!(!logo.used_alternate);
        assert!(
            server.finish().is_empty(),
            "legacy cache made an HTTP request"
        );
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn commented_xml_svg_is_accepted_when_served_as_plain_text() {
        const SVG: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<!-- Official logo artwork -->
<!DOCTYPE svg [<!ELEMENT svg ANY>]>
<svg xmlns="http://www.w3.org/2000/svg"></svg>"#;
        let server = Server::start(vec![Response {
            status: "200 OK",
            headers: vec![("Content-Type", "text/plain")],
            body: SVG,
        }]);
        let directory = cache_dir("svg-text");
        let primary = server.url("primary");

        let logo = load_logo(&client(), &directory, 10, &primary, &[], false).await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(SVG, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_none());
        server.finish();
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
