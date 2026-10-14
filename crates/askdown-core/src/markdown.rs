//! Turns the Markdown around the questions into HTML for the window.

use pulldown_cmark::{Options, Parser, html};

/// Markdown to HTML, sanitised.
///
/// The file is written by an AI agent and shown in a webview that can call back
/// into the app, so script, event handlers and the like are stripped. `<video>`
/// and `<audio>` are allowed because playing media in a question is part of the point.
pub fn to_html(markdown: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut raw = String::new();
    html::push_html(&mut raw, Parser::new_ext(markdown, options));

    ammonia::Builder::default()
        .add_tags(["video", "audio", "source"])
        .add_tag_attributes("video", ["src", "controls", "loop", "muted", "poster", "width", "height"])
        .add_tag_attributes("audio", ["src", "controls", "loop"])
        .add_tag_attributes("source", ["src", "type"])
        .clean(&raw)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_media_and_links_but_drops_script() {
        let html = to_html(
            "[link](https://example.com)\n\n<video src=\"a.mp4\" controls></video>\n\n<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>",
        );
        assert!(html.contains("href=\"https://example.com\""));
        assert!(html.contains("<video"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("onerror"));
    }
}
