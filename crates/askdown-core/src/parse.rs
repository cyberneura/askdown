//! Finds the question blocks in a Markdown document.
//!
//! A question is a fenced code block whose info string is `askdown`. Its answer,
//! once given, sits right after it between `<!-- askdown:answer -->` markers.
//! Everything is located by byte offset, so writing the answers back replaces
//! those ranges only and leaves the rest of the file exactly as written.

use std::ops::Range;

use crate::question::Question;

pub const ANSWER_START: &str = "<!-- askdown:answer";
pub const ANSWER_END: &str = "<!-- /askdown:answer -->";

/// One question block and where it sits in the source.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// 1-based, in the order the blocks appear.
    pub number: usize,
    /// The question, or why its YAML could not be read.
    pub question: Result<Question, String>,
    /// From the opening fence to the end of the closing fence's line.
    pub range: Range<usize>,
    /// The answer block already written for it, if any.
    pub answer_range: Option<Range<usize>>,
}

impl Block {
    /// The key the answer is reported under.
    pub fn id(&self) -> String {
        match &self.question {
            Ok(Question { id: Some(id), .. }) => id.clone(),
            _ => format!("q{}", self.number),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    /// Markdown between question blocks, shown as it is.
    Prose(String),
    /// Index into [`Document::blocks`].
    Question(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub segments: Vec<Segment>,
    pub blocks: Vec<Block>,
}

struct Fence {
    marker: u8,
    length: usize,
}

/// The opening fence of a code block on this line, with its info string.
fn opening_fence(line: &str) -> Option<(Fence, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let marker = *rest.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let length = rest.bytes().take_while(|&byte| byte == marker).count();
    if length < 3 {
        return None;
    }
    let info = rest[length..].trim();
    if marker == b'`' && info.contains('`') {
        return None;
    }
    Some((Fence { marker, length }, info))
}

fn closes(fence: &Fence, line: &str) -> bool {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return false;
    }
    let length = trimmed.bytes().take_while(|&byte| byte == fence.marker).count();
    length >= fence.length && trimmed[length..].trim().is_empty()
}

/// Lines of `source` with the byte offset each starts at. The line keeps its ending.
fn lines_with_offsets(source: &str) -> Vec<(usize, &str)> {
    let mut offset = 0;
    source
        .split_inclusive('\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            (start, line)
        })
        .collect()
}

pub fn parse(source: &str) -> Document {
    let lines = lines_with_offsets(source);
    let mut blocks = Vec::new();
    let mut segments = Vec::new();
    let mut prose_start = 0;
    let mut index = 0;

    while index < lines.len() {
        let (start, line) = lines[index];
        let Some((fence, info)) = opening_fence(line.trim_end_matches(['\n', '\r'])) else {
            index += 1;
            continue;
        };
        // The body runs to the closing fence, or to the end of the document when
        // there is none -- as CommonMark has it.
        let mut close = index + 1;
        while close < lines.len() && !closes(&fence, lines[close].1.trim_end_matches(['\n', '\r'])) {
            close += 1;
        }
        let body_start = start + line.len();
        let body_end = lines.get(close).map_or(source.len(), |(offset, _)| *offset);
        let end = lines
            .get(close)
            .map_or(source.len(), |(offset, line)| offset + line.len());

        // Only the first word counts, so `askdown title="..."` style extras stay possible.
        if info.split_whitespace().next() != Some("askdown") {
            index = close + 1;
            continue;
        }

        let answer_range = answer_after(source, &lines, close + 1);
        push_prose(&mut segments, &source[prose_start..start]);
        segments.push(Segment::Question(blocks.len()));
        blocks.push(Block {
            number: blocks.len() + 1,
            question: Question::from_yaml(&source[body_start..body_end]),
            range: start..end,
            answer_range: answer_range.clone(),
        });
        prose_start = answer_range.map_or(end, |range| range.end);
        index = lines
            .iter()
            .position(|(offset, _)| *offset >= prose_start)
            .unwrap_or(lines.len());
    }
    push_prose(&mut segments, &source[prose_start..]);
    Document { segments, blocks }
}

fn push_prose(segments: &mut Vec<Segment>, text: &str) {
    if !text.trim().is_empty() {
        segments.push(Segment::Prose(text.to_string()));
    }
}

/// The answer block that follows a question, allowing blank lines in between.
fn answer_after(source: &str, lines: &[(usize, &str)], from: usize) -> Option<Range<usize>> {
    let mut index = from;
    while index < lines.len() && lines[index].1.trim().is_empty() {
        index += 1;
    }
    let (start, line) = *lines.get(index)?;
    if !line.trim_start().starts_with(ANSWER_START) {
        return None;
    }
    // An opening marker with no end is left alone: guessing where it stops could
    // swallow the user's text on the next write-back.
    let end_line = lines[index..]
        .iter()
        .find(|(_, line)| line.trim() == ANSWER_END)?;
    let end = (end_line.0 + end_line.1.len()).min(source.len());
    Some(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_question_blocks_and_keeps_the_prose_between_them() {
        let document = parse(
            "# Title\n\nIntro\n\n```askdown\ntype: text\n```\n\nMiddle\n\n```sh\nls\n```\n\n~~~askdown\ntype: color\n~~~\n",
        );
        assert_eq!(document.blocks.len(), 2);
        assert_eq!(document.blocks[0].id(), "q1");
        assert_eq!(document.blocks[1].number, 2);
        assert!(matches!(&document.segments[0], Segment::Prose(text) if text.contains("Intro")));
        assert!(matches!(&document.segments[2], Segment::Prose(text) if text.contains("```sh")));
    }

    #[test]
    fn a_broken_question_is_reported_rather_than_dropped() {
        let document = parse("```askdown\ntype: [\n```\n");
        assert_eq!(document.blocks.len(), 1);
        assert!(document.blocks[0].question.is_err());
    }

    #[test]
    fn an_askdown_block_inside_a_longer_fence_is_not_a_question() {
        let document = parse("````md\n```askdown\ntype: text\n```\n````\n");
        assert!(document.blocks.is_empty());
    }

    #[test]
    fn finds_the_answer_written_after_a_question() {
        let source = "```askdown\ntype: text\n```\n\n<!-- askdown:answer -->\n```yaml\nvalue: hi\n```\n<!-- /askdown:answer -->\n\nAfter\n";
        let document = parse(source);
        let range = document.blocks[0].answer_range.clone().unwrap();
        assert!(source[range].ends_with("<!-- /askdown:answer -->\n"));
        assert!(matches!(document.segments.last(), Some(Segment::Prose(text)) if text.trim() == "After"));
    }

    #[test]
    fn an_unterminated_answer_marker_is_not_treated_as_an_answer() {
        let document = parse("```askdown\ntype: text\n```\n<!-- askdown:answer -->\nmine\n");
        assert_eq!(document.blocks[0].answer_range, None);
    }
}
