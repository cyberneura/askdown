//! Serves the files next to the Markdown to the window.
//!
//! The window loads images, video and audio through its own URI scheme
//! (`askdown://localhost/<path>`, or `http://askdown.localhost/<path>` on
//! Windows), and this module answers those requests with files from the
//! Markdown file's directory. Nothing outside that directory is served: the
//! Markdown is written by an AI agent, so a path in it is not trusted to stay
//! put. Range requests are honoured because WebKit asks for them to seek video.

use std::borrow::Cow;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use tauri::http::{Request, Response, StatusCode, header};

/// The protocol's scheme name.
pub const SCHEME: &str = "askdown";

/// Where the window reaches the protocol, with a trailing slash, so that a
/// relative path appended to it makes a URL.
pub fn base_url() -> &'static str {
    if cfg!(windows) {
        "http://askdown.localhost/"
    } else {
        "askdown://localhost/"
    }
}

type Body = Cow<'static, [u8]>;

/// Answers a request for a file in `dir`. `dir` must be canonical.
pub fn respond(dir: &Path, request: &Request<Vec<u8>>) -> Response<Body> {
    let file = match resolve(dir, request.uri().path()) {
        Ok(file) => file,
        Err(code) => return status(code),
    };
    let Ok(mut handle) = File::open(&file) else {
        return status(StatusCode::NOT_FOUND);
    };
    let Ok(metadata) = handle.metadata() else {
        return status(StatusCode::NOT_FOUND);
    };
    if !metadata.is_file() {
        return status(StatusCode::NOT_FOUND);
    }
    let length = metadata.len();

    // The type is sniffed from the start of the file whatever part is asked
    // for: the middle of a video says nothing about what it is.
    let mut head = vec![0; length.min(8192) as usize];
    if handle.read_exact(&mut head).is_err() {
        return status(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let mime = tauri::utils::mime_type::MimeType::parse_with_fallback(
        &head,
        &file.to_string_lossy(),
        tauri::utils::mime_type::MimeType::OctetStream,
    );

    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(|value| parse_range(value, length));
    let (start, end, code) = match range {
        None => (0, length, StatusCode::OK),
        Some(Some((start, end))) => (start, end, StatusCode::PARTIAL_CONTENT),
        Some(None) => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{length}"))
                .body(Body::default())
                .expect("static headers");
        }
    };

    let mut body = Vec::with_capacity((end - start) as usize);
    if handle.seek(SeekFrom::Start(start)).is_err()
        || handle.take(end - start).read_to_end(&mut body).is_err()
    {
        return status(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let mut response = Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, body.len())
        .header(header::ACCEPT_RANGES, "bytes")
        // The window's origin is a different one, and the canvas of a `draw`
        // question would be tainted by a background image without this.
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    if code == StatusCode::PARTIAL_CONTENT {
        response = response.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{length}", end - 1),
        );
    }
    response.body(body.into()).expect("static headers")
}

fn status(code: StatusCode) -> Response<Body> {
    Response::builder()
        .status(code)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(Body::default())
        .expect("static headers")
}

/// The file a request path points at, if it is inside `dir`.
///
/// `..`, absolute paths and symlinks leading out of the directory are refused;
/// a path that does not exist is not found. The result is canonical.
fn resolve(dir: &Path, request_path: &str) -> Result<PathBuf, StatusCode> {
    let decoded = percent_encoding::percent_decode_str(request_path.trim_start_matches('/'))
        .decode_utf8()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let relative = Path::new(decoded.as_ref());
    if !relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(StatusCode::FORBIDDEN);
    }
    let file = dir
        .join(relative)
        .canonicalize()
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if file.starts_with(dir) {
        Ok(file)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// `bytes=start-end` as a half-open `start..end` clamped to `length`.
///
/// Only a single range is understood, which is all a media player sends.
/// `None` means the range cannot be satisfied.
fn parse_range(value: &str, length: u64) -> Option<(u64, u64)> {
    let spec = value.strip_prefix("bytes=")?.trim();
    let (start, end) = spec.split_once('-')?;
    let (start, end) = match (start.trim(), end.trim()) {
        ("", suffix) => {
            let count: u64 = suffix.parse().ok()?;
            (length.saturating_sub(count), length)
        }
        (start, "") => (start.parse().ok()?, length),
        (start, end) => {
            let end: u64 = end.parse().ok()?;
            (start.parse().ok()?, end.checked_add(1)?.min(length))
        }
    };
    (start < end && start < length).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of its own per test: they run in parallel and must not
    /// rewrite each other's files.
    fn dir(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("askdown-media-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), b"0123456789").unwrap();
        std::fs::write(dir.join("sub").join("b.png"), b"\x89PNG\r\n\x1a\n....").unwrap();
        dir.canonicalize().unwrap()
    }

    fn request(path: &str, range: Option<&str>) -> Request<Vec<u8>> {
        let mut builder = Request::builder().uri(format!("askdown://localhost{path}"));
        if let Some(range) = range {
            builder = builder.header(header::RANGE, range);
        }
        builder.body(Vec::new()).unwrap()
    }

    #[test]
    fn serves_a_file_inside_the_directory() {
        let response = respond(
            &dir("serves_a_file_inside_the_directory"),
            &request("/sub/b.png", None),
        );
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
        assert!(response.body().starts_with(b"\x89PNG"));
    }

    #[test]
    fn decodes_percent_encoding() {
        let dir = dir("decodes_percent_encoding");
        std::fs::write(dir.join("with space.txt"), b"x").unwrap();
        let response = respond(&dir, &request("/with%20space.txt", None));
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn refuses_to_leave_the_directory() {
        let dir = dir("refuses_to_leave_the_directory");
        // Extra leading slashes do not make the path absolute: it stays under `dir`.
        assert!(resolve(&dir, "//sub/b.png").is_ok());
        for path in ["/../a.txt", "/sub/../../a.txt", "/%2e%2e/a.txt"] {
            assert_eq!(resolve(&dir, path), Err(StatusCode::FORBIDDEN), "{path}");
            assert_eq!(
                respond(&dir, &request(path, None)).status(),
                StatusCode::FORBIDDEN,
                "{path}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlink_that_leads_outside() {
        let dir = dir("refuses_a_symlink_that_leads_outside");
        let link = dir.join("escape");
        std::os::unix::fs::symlink("/", &link).unwrap();
        assert_eq!(
            resolve(&dir, "/escape/etc/hosts"),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn a_missing_file_is_not_found() {
        assert_eq!(
            respond(
                &dir("a_missing_file_is_not_found"),
                &request("/nope.txt", None)
            )
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            respond(&dir("a_missing_file_is_not_found"), &request("/sub", None)).status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn answers_a_range_with_partial_content() {
        let response = respond(
            &dir("answers_a_range_with_partial_content"),
            &request("/a.txt", Some("bytes=2-4")),
        );
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body().as_ref(), b"234");
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 2-4/10");
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "3");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/plain");
    }

    #[test]
    fn parses_open_ended_and_suffix_ranges() {
        assert_eq!(parse_range("bytes=7-", 10), Some((7, 10)));
        assert_eq!(parse_range("bytes=-3", 10), Some((7, 10)));
        assert_eq!(parse_range("bytes=0-", 10), Some((0, 10)));
        assert_eq!(parse_range("bytes=2-100", 10), Some((2, 10)));
        assert_eq!(parse_range("bytes=10-", 10), None);
        assert_eq!(parse_range("bytes=5-4", 10), None);
        assert_eq!(parse_range("items=0-1", 10), None);
    }

    #[test]
    fn an_unsatisfiable_range_says_so() {
        let response = respond(
            &dir("an_unsatisfiable_range_says_so"),
            &request("/a.txt", Some("bytes=20-30")),
        );
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes */10");
    }
}
