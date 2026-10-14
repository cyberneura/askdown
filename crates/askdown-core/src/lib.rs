//! Core of askdown.
//!
//! Finds the questions written in Markdown (`parse`), describes them (`question`),
//! renders the prose around them (`markdown`), and formats the answers for writing
//! back into the same Markdown (`answer`).
//!
//! File IO is deliberately kept out of this crate. The askdown binary performs it,
//! so that everything here stays pure and testable.

pub mod answer;
pub mod markdown;
pub mod parse;
pub mod question;

pub use answer::{Answer, AnswerSet, write_back};
pub use parse::{Block, Document, Segment, parse};
pub use question::{ChoiceOption, Question, QuestionKind};
