//! Answers, and writing them back into the Markdown.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::parse::{ANSWER_END, ANSWER_START, Document};

/// What the user gave for one question. The shape depends on the question's kind
/// and is decided by the window, so it is kept as plain JSON here.
pub type Answer = serde_json::Value;

/// Answers keyed by question id, in a stable order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnswerSet(pub BTreeMap<String, Answer>);

/// The answer block written after a question.
///
/// YAML rather than JSON: the file is read by people as well as agents, and the
/// agent gets JSON on stdout anyway.
pub fn render_answer(answer: &Answer) -> String {
    let yaml = serde_saphyr::to_string(answer).unwrap_or_else(|_| answer.to_string());
    let yaml = yaml.trim_end();
    format!("{ANSWER_START} -->\n```yaml\n{yaml}\n```\n{ANSWER_END}\n")
}

/// The value inside an answer block written by [`render_answer`].
pub fn read_answer(block: &str) -> Option<Answer> {
    let body = block.split_once("```yaml\n")?.1.rsplit_once("\n```")?.0;
    serde_saphyr::from_str(body).ok()
}

/// `source` with every answered question's answer block written or replaced.
///
/// `document` must be the result of parsing `source`. Questions without an
/// answer in `answers` keep whatever block they had.
pub fn write_back(source: &str, document: &Document, answers: &AnswerSet) -> String {
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for block in &document.blocks {
        let Some(answer) = answers.0.get(&block.id()) else {
            continue;
        };
        let rendered = render_answer(answer);
        match &block.answer_range {
            Some(range) => edits.push((range.clone(), rendered)),
            None => {
                // The closing fence may be the last line with no newline after it.
                let separator = if source[..block.range.end].ends_with('\n') { "\n" } else { "\n\n" };
                edits.push((block.range.end..block.range.end, format!("{separator}{rendered}")));
            }
        }
    }
    // Applied back to front so that earlier offsets stay valid.
    let mut output = source.to_string();
    for (range, text) in edits.into_iter().rev() {
        output.replace_range(range, &text);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use serde_json::json;

    fn answers(pairs: &[(&str, Answer)]) -> AnswerSet {
        AnswerSet(pairs.iter().map(|(id, value)| (id.to_string(), value.clone())).collect())
    }

    #[test]
    fn inserts_an_answer_after_the_question() {
        let source = "Before\n\n```askdown\ntype: text\n```\n\nAfter\n";
        let output = write_back(source, &parse(source), &answers(&[("q1", json!("hello"))]));
        assert!(output.starts_with("Before\n\n```askdown\ntype: text\n```\n\n<!-- askdown:answer -->\n```yaml\nhello\n```\n<!-- /askdown:answer -->\n"));
        assert!(output.ends_with("\nAfter\n"));
    }

    #[test]
    fn answering_again_replaces_the_previous_answer() {
        let source = "```askdown\ntype: text\n```\n";
        let once = write_back(source, &parse(source), &answers(&[("q1", json!("first"))]));
        let twice = write_back(&once, &parse(&once), &answers(&[("q1", json!("second"))]));
        assert!(!twice.contains("first"));
        assert_eq!(twice.matches(ANSWER_END).count(), 1);
    }

    #[test]
    fn handles_a_question_at_the_very_end_without_a_newline() {
        let source = "```askdown\ntype: text\n```";
        let output = write_back(source, &parse(source), &answers(&[("q1", json!("x"))]));
        assert!(output.starts_with("```askdown\ntype: text\n```\n\n<!-- askdown:answer -->"));
        assert_eq!(parse(&output).blocks[0].answer_range.is_some(), true);
    }

    #[test]
    fn reads_back_what_it_wrote() {
        let answer = json!({ "selected": ["a", "b"], "other": "x: y" });
        assert_eq!(read_answer(&render_answer(&answer)), Some(answer));
    }

    #[test]
    fn leaves_unanswered_questions_alone() {
        let source = "```askdown\ntype: text\n```\n\n```askdown\nid: b\ntype: text\n```\n";
        let output = write_back(source, &parse(source), &answers(&[("b", json!(1))]));
        assert_eq!(output.matches(ANSWER_END).count(), 1);
        assert!(output.find(ANSWER_END).unwrap() > output.find("id: b").unwrap());
    }
}
