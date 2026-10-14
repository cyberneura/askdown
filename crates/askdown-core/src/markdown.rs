//! Turns the Markdown around the questions into HTML for the window.

use std::borrow::Cow;

use pulldown_cmark::{Options, Parser, html};

/// The URL a relative path in the Markdown is rewritten to.
///
/// `base` is where the window serves the files next to the Markdown from
/// (something like `askdown://localhost/`); `path` is as written in the file.
/// Anchors and absolute URLs are not touched. Nothing is normalised here: the
/// webview resolves `..` when it parses the URL, and whatever reaches the server
/// is only ever looked up inside the Markdown file's directory.
pub fn media_url(base: &str, path: &str) -> String {
    let path = path.strip_prefix("./").unwrap_or(path);
    format!("{base}{path}")
}

/// Markdown to HTML, sanitised.
///
/// The file is written by an AI agent and shown in a webview that can call back
/// into the app, so script, event handlers and the like are stripped. `<video>`
/// and `<audio>` are allowed because playing media in a question is part of the point.
///
/// Relative URLs are rewritten with [`media_url`] so that images and media next
/// to the Markdown file load through the window's own file protocol.
pub fn to_html(markdown: &str, media_base: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut raw = String::new();
    html::push_html(&mut raw, Parser::new_ext(markdown, options));

    let scheme = media_base.split_once(':').map_or("", |(scheme, _)| scheme);
    let rewrite = Rewrite {
        base: media_base.to_string(),
    };
    ammonia::Builder::default()
        .add_tags(["video", "audio", "source"])
        .add_tag_attributes(
            "video",
            [
                "src", "controls", "loop", "muted", "poster", "width", "height",
            ],
        )
        .add_tag_attributes("audio", ["src", "controls", "loop"])
        .add_tag_attributes("source", ["src", "type"])
        .add_url_schemes([scheme])
        // Applies to `href`, `src` and a video's `poster`.
        .url_relative(ammonia::UrlRelative::Custom(Box::new(rewrite)))
        .clean(&raw)
        .to_string()
}

/// Rewrites relative URLs to the media base.
struct Rewrite {
    base: String,
}

impl Rewrite {
    fn rewrite<'u>(&self, url: &'u str) -> Cow<'u, str> {
        if url.starts_with('#') || url.starts_with('/') {
            Cow::Borrowed(url)
        } else {
            Cow::Owned(media_url(&self.base, url))
        }
    }
}

impl ammonia::UrlRelativeEvaluate<'_> for Rewrite {
    fn evaluate<'u>(&self, url: &'u str) -> Option<Cow<'u, str>> {
        Some(self.rewrite(url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "askdown://localhost/";

    #[test]
    fn keeps_media_and_links_but_drops_script() {
        let html = to_html(
            "[link](https://example.com)\n\n<video src=\"https://example.com/a.mp4\" controls></video>\n\n<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>",
            BASE,
        );
        assert!(html.contains("href=\"https://example.com\""));
        assert!(html.contains("<video"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("onerror"));
    }

    #[test]
    fn relative_paths_are_served_through_the_media_base() {
        let html = to_html(
            "![shot](./shots/a.png)\n\n<video src=\"clip.mp4\" poster=\"clip.jpg\" controls></video>\n\n<audio src=\"../up.mp3\"></audio>",
            BASE,
        );
        assert!(html.contains("src=\"askdown://localhost/shots/a.png\""));
        assert!(html.contains("src=\"askdown://localhost/clip.mp4\""));
        assert!(html.contains("poster=\"askdown://localhost/clip.jpg\""));
        // Not normalised here; the webview resolves it before asking the server.
        assert!(html.contains("src=\"askdown://localhost/../up.mp3\""));
    }

    #[test]
    fn absolute_urls_and_anchors_are_left_alone() {
        let html = to_html(
            "[a](https://example.com/x)\n[b](#section)\n![c](/abs.png)\n[d](mailto:x@example.com)",
            BASE,
        );
        assert!(html.contains("href=\"https://example.com/x\""));
        assert!(html.contains("href=\"#section\""));
        assert!(html.contains("src=\"/abs.png\""));
        assert!(html.contains("href=\"mailto:x@example.com\""));
    }

    #[test]
    fn the_windows_form_of_the_base_works_too() {
        let html = to_html("![](a.png)", "http://askdown.localhost/");
        assert!(html.contains("src=\"http://askdown.localhost/a.png\""));
    }
}
