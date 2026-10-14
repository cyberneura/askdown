//! What a question block says, once its YAML has been read.

use serde::{Deserialize, Serialize};

/// The kind of input a question asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuestionKind {
    /// Pick from text options. `multiple` allows more than one; `other` adds a free-text field.
    Choice,
    /// Free text.
    Text,
    /// Pick from images or videos.
    MediaChoice,
    /// Point at a spot on an image or a video frame.
    Point,
    /// Draw freehand, optionally over a background image.
    Draw,
    /// Pick a colour.
    Color,
    /// Mark a span of an audio or video file.
    Range,
}

/// One option of a `choice` or `media-choice` question.
///
/// Written either as a bare string (`- staging`) or as a mapping
/// (`- { label: Staging, value: stg, src: ./a.png }`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChoiceOption {
    Plain(String),
    Detailed {
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        value: Option<String>,
        /// Image or video shown for the option. Relative paths are relative to the Markdown file.
        #[serde(default)]
        src: Option<String>,
        #[serde(default)]
        description: Option<String>,
    },
}

impl ChoiceOption {
    /// The value written into the answer.
    pub fn value(&self) -> &str {
        match self {
            ChoiceOption::Plain(text) => text,
            ChoiceOption::Detailed {
                value, label, src, ..
            } => value
                .as_deref()
                .or(label.as_deref())
                .or(src.as_deref())
                .unwrap_or(""),
        }
    }

    /// The text shown to the user.
    pub fn label(&self) -> &str {
        match self {
            ChoiceOption::Plain(text) => text,
            ChoiceOption::Detailed { label, .. } => {
                label.as_deref().unwrap_or_else(|| self.value())
            }
        }
    }
}

/// A question as written in an ```` ```askdown ```` block.
///
/// Fields that a kind does not use are accepted and ignored, so that one shape
/// serves every kind and the YAML stays forgiving to write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    /// Key the answer is reported under. Defaults to `q<number>`.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: QuestionKind,
    /// The question itself, in Markdown.
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub multiple: bool,
    /// For `choice`: also offer a free-text field.
    #[serde(default)]
    pub other: bool,
    #[serde(default)]
    pub options: Vec<ChoiceOption>,
    /// Media the question is about (`point`, `range`) or drawn over (`draw`).
    #[serde(default)]
    pub src: Option<String>,
    /// For `text`: hint shown in the empty field.
    #[serde(default)]
    pub placeholder: Option<String>,
    /// For `text`: a multi-line field instead of a single line.
    #[serde(default)]
    pub multiline: bool,
    /// Pre-filled value.
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    /// For `draw` without `src`: the size of the blank canvas, in pixels.
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

impl Question {
    pub fn from_yaml(source: &str) -> Result<Question, String> {
        serde_saphyr::from_str(source).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_choice_question_with_plain_and_detailed_options() {
        let question = Question::from_yaml(
            "type: choice\nquestion: Where?\nmultiple: true\noptions:\n  - staging\n  - { label: Production, value: prod }\n",
        )
        .unwrap();
        assert_eq!(question.kind, QuestionKind::Choice);
        assert!(question.multiple);
        assert_eq!(question.options[0].value(), "staging");
        assert_eq!(question.options[1].value(), "prod");
        assert_eq!(question.options[1].label(), "Production");
    }

    #[test]
    fn rejects_a_misspelt_field_instead_of_ignoring_it() {
        // A typo such as `mutliple` would otherwise silently turn a multi-select
        // question into a single-select one.
        assert!(Question::from_yaml("type: choice\nmutliple: true\n").is_err());
    }

    #[test]
    fn rejects_an_unknown_kind() {
        assert!(Question::from_yaml("type: slider\n").is_err());
    }
}
