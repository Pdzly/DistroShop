use base64::{encoded_len, engine::general_purpose::STANDARD, Engine as _};
use reqwest::header::{HeaderValue, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Default)]
pub(crate) struct CachedLogo {
    pub source: Option<Arc<str>>,
    pub warning: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CacheEntry {
    url: String,
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

fn cached_logo(entry: CacheEntry, warning: Option<String>) -> CachedLogo {
    CachedLogo {
        source: Some(Arc::from(entry.data_url)),
        warning,
    }
}

fn warning_with_stale_cache(message: String, stale: Option<CacheEntry>) -> CachedLogo {
    match stale {
        Some(entry) => cached_logo(entry, Some(message)),
        None => CachedLogo {
            source: None,
            warning: Some(message),
        },
    }
}

/// Loads a distro logo from the persistent cache or downloads it when needed.
///
/// A non-refresh call never makes a request when it has a valid cache entry for
/// the same URL. Refreshes use the saved validators only for that same URL.
pub(crate) async fn load_logo(
    client: &reqwest::Client,
    cache_dir: &Path,
    id: u8,
    url: &str,
    refresh: bool,
) -> CachedLogo {
    let path = cache_path(cache_dir, id);
    let cached = read_cache(&path);

    if !refresh && cached.as_ref().is_some_and(|entry| entry.url == url) {
        return cached_logo(cached.expect("checked above"), None);
    }

    let same_url = cached.as_ref().is_some_and(|entry| entry.url == url);
    let mut request = client.get(url);
    if same_url {
        if let Some(etag) = cached.as_ref().and_then(|entry| entry.etag.as_deref()) {
            if let Ok(value) = HeaderValue::from_str(etag) {
                request = request.header(IF_NONE_MATCH, value);
            }
        }
        if let Some(last_modified) = cached
            .as_ref()
            .and_then(|entry| entry.last_modified.as_deref())
        {
            if let Ok(value) = HeaderValue::from_str(last_modified) {
                request = request.header(IF_MODIFIED_SINCE, value);
            }
        }
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            return warning_with_stale_cache(
                format!("Could not download distro logo: {error}"),
                cached,
            );
        }
    };

    if response.status() == reqwest::StatusCode::NOT_MODIFIED && same_url {
        return cached_logo(cached.expect("same URL has cache"), None);
    }
    if !response.status().is_success() {
        return warning_with_stale_cache(
            format!(
                "Could not download distro logo: server returned {}",
                response.status()
            ),
            cached,
        );
    }

    let etag = header_value(response.headers(), ETAG);
    let last_modified = header_value(response.headers(), LAST_MODIFIED);
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            return warning_with_stale_cache(
                format!("Could not read distro logo: {error}"),
                cached,
            );
        }
    };
    let Some(mime_type) = image_mime_type(&bytes) else {
        return warning_with_stale_cache(
            "Could not download distro logo: response was not an image".to_owned(),
            cached,
        );
    };

    let data_url = data_url(&bytes, mime_type);
    if cached
        .as_ref()
        .is_some_and(|entry| entry.url == url && entry.data_url == data_url)
    {
        let mut entry = cached.expect("checked above");
        if entry.etag == etag && entry.last_modified == last_modified {
            return cached_logo(entry, None);
        }
        entry.etag = etag;
        entry.last_modified = last_modified;
        if let Err(error) = write_cache(&path, &entry) {
            return CachedLogo {
                source: Some(Arc::from(entry.data_url)),
                warning: Some(format!("Could not save distro logo cache: {error}")),
            };
        }
        return cached_logo(entry, None);
    }

    let entry = CacheEntry {
        url: url.to_owned(),
        data_url,
        etag,
        last_modified,
    };
    if let Err(error) = write_cache(&path, &entry) {
        return CachedLogo {
            source: Some(Arc::from(entry.data_url)),
            warning: Some(format!("Could not save distro logo cache: {error}")),
        };
    }
    cached_logo(entry, None)
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
        time::Duration,
    };

    const IMAGE_ONE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="red"/></svg>"#;
    const IMAGE_TWO: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="blue"/></svg>"#;

    struct Response {
        status: &'static str,
        headers: Vec<(&'static str, &'static str)>,
        body: &'static [u8],
    }

    struct Server {
        url: String,
        requests: Arc<Mutex<Vec<String>>>,
        worker: thread::JoinHandle<()>,
    }

    impl Server {
        fn start(responses: Vec<Response>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
            listener
                .set_nonblocking(true)
                .expect("set fixture nonblocking");
            let url = format!(
                "http://{}/logo",
                listener.local_addr().expect("local address")
            );
            let requests = Arc::new(Mutex::new(Vec::new()));
            let captured = Arc::clone(&requests);
            let worker = thread::spawn(move || {
                let mut responses = VecDeque::from(responses);
                while let Some(response) = responses.pop_front() {
                    let deadline = std::time::Instant::now() + Duration::from_secs(3);
                    let mut stream = loop {
                        match listener.accept() {
                            Ok((stream, _)) => break stream,
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(
                                    std::time::Instant::now() < deadline,
                                    "expected image request"
                                );
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
                url,
                requests,
                worker,
            }
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

    fn write_test_cache(directory: &Path, id: u8, url: &str, bytes: &[u8], etag: Option<&str>) {
        write_cache(
            &cache_path(directory, id),
            &CacheEntry {
                url: url.to_owned(),
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
            .timeout(Duration::from_secs(1))
            .build()
            .expect("build client")
    }

    #[tokio::test]
    async fn startup_reuses_matching_cache_without_a_request() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused address");
        let url = format!(
            "http://{}/logo",
            listener.local_addr().expect("local address")
        );
        let directory = cache_dir("startup");
        write_test_cache(&directory, 3, &url, IMAGE_ONE, Some("old"));

        let logo = load_logo(&client(), &directory, 3, &url, false).await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_ONE, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_none());
        listener.set_nonblocking(true).expect("set nonblocking");
        assert!(
            listener.accept().is_err(),
            "cache reuse made an HTTP request"
        );
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn refresh_reuses_cache_after_conditional_not_modified() {
        let server = Server::start(vec![Response {
            status: "304 Not Modified",
            headers: vec![],
            body: b"",
        }]);
        let directory = cache_dir("not-modified");
        write_test_cache(&directory, 4, &server.url, IMAGE_ONE, Some("logo-v1"));

        let logo = load_logo(&client(), &directory, 4, &server.url, true).await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_ONE, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_none());
        let requests = server.finish();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("if-none-match: logo-v1\r\n"));
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn refresh_replaces_changed_image_for_same_url() {
        let server = Server::start(vec![Response {
            status: "200 OK",
            headers: vec![("ETag", "logo-v2")],
            body: IMAGE_TWO,
        }]);
        let directory = cache_dir("changed-image");
        write_test_cache(&directory, 5, &server.url, IMAGE_ONE, Some("logo-v1"));

        let logo = load_logo(&client(), &directory, 5, &server.url, true).await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_TWO, "image/svg+xml").as_str())
        );
        let stored = read_cache(&cache_path(&directory, 5)).expect("updated cache");
        assert_eq!(stored.data_url, data_url(IMAGE_TWO, "image/svg+xml"));
        assert_eq!(stored.etag.as_deref(), Some("logo-v2"));
        server.finish();
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn successful_refresh_with_identical_content_updates_validators() {
        let server = Server::start(vec![Response {
            status: "200 OK",
            headers: vec![("ETag", "logo-v2")],
            body: IMAGE_ONE,
        }]);
        let directory = cache_dir("unchanged-image");
        write_test_cache(&directory, 12, &server.url, IMAGE_ONE, Some("logo-v1"));

        let logo = load_logo(&client(), &directory, 12, &server.url, true).await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_ONE, "image/svg+xml").as_str())
        );
        assert_eq!(
            read_cache(&cache_path(&directory, 12))
                .expect("existing cache")
                .etag
                .as_deref(),
            Some("logo-v2")
        );
        server.finish();
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn changed_url_does_not_send_old_validators() {
        let server = Server::start(vec![Response {
            status: "200 OK",
            headers: vec![],
            body: IMAGE_TWO,
        }]);
        let directory = cache_dir("changed-url");
        write_test_cache(
            &directory,
            6,
            "http://old.invalid/logo",
            IMAGE_ONE,
            Some("old-validator"),
        );

        let logo = load_logo(&client(), &directory, 6, &server.url, true).await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_TWO, "image/svg+xml").as_str())
        );
        let requests = server.finish();
        assert!(!requests[0].contains("if-none-match:"));
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn missing_or_corrupt_cache_fetches_a_logo() {
        let server = Server::start(vec![
            Response {
                status: "200 OK",
                headers: vec![],
                body: IMAGE_ONE,
            },
            Response {
                status: "200 OK",
                headers: vec![],
                body: IMAGE_TWO,
            },
        ]);
        let directory = cache_dir("missing-corrupt");

        let missing = load_logo(&client(), &directory, 7, &server.url, false).await;
        fs::write(cache_path(&directory, 8), b"not json").expect("write corrupt cache");
        let corrupt = load_logo(&client(), &directory, 8, &server.url, false).await;

        assert_eq!(
            missing.source.as_deref(),
            Some(data_url(IMAGE_ONE, "image/svg+xml").as_str())
        );
        assert_eq!(
            corrupt.source.as_deref(),
            Some(data_url(IMAGE_TWO, "image/svg+xml").as_str())
        );
        assert!(missing.warning.is_none());
        assert!(corrupt.warning.is_none());
        assert_eq!(server.finish().len(), 2);
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn failed_refresh_after_url_change_keeps_the_old_logo() {
        let server = Server::start(vec![Response {
            status: "500 Server Error",
            headers: vec![],
            body: b"failure",
        }]);
        let directory = cache_dir("failed-refresh");
        write_test_cache(&directory, 9, "http://old.invalid/logo", IMAGE_ONE, None);

        let logo = load_logo(&client(), &directory, 9, &server.url, true).await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_ONE, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_some());
        assert_eq!(
            read_cache(&cache_path(&directory, 9))
                .expect("old cache")
                .data_url,
            data_url(IMAGE_ONE, "image/svg+xml")
        );
        server.finish();
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn non_image_response_is_rejected_without_replacing_cache() {
        let server = Server::start(vec![Response {
            status: "200 OK",
            headers: vec![("Content-Type", "text/plain")],
            body: b"not an image",
        }]);
        let directory = cache_dir("non-image");
        write_test_cache(&directory, 10, &server.url, IMAGE_ONE, None);

        let logo = load_logo(&client(), &directory, 10, &server.url, true).await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(IMAGE_ONE, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_some());
        assert_eq!(
            read_cache(&cache_path(&directory, 10))
                .expect("old cache")
                .data_url,
            data_url(IMAGE_ONE, "image/svg+xml")
        );
        server.finish();
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

        let logo = load_logo(&client(), &directory, 11, &server.url, false).await;

        assert_eq!(
            logo.source.as_deref(),
            Some(data_url(SVG, "image/svg+xml").as_str())
        );
        assert!(logo.warning.is_none());
        server.finish();
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
