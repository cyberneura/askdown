# askdown

Ask a person questions from a Markdown file, in a desktop window.

An AI agent writes its questions into a Markdown file and runs `askdown` on it.
A window opens showing the Markdown (links, images, video) with an input for each
question. When the user presses **Submit**, the answers are printed to stdout as
JSON and written back into the file under each question. The command blocks until
then, so it works as an `AskUserQuestion` for agents that do not have one, such as Codex.

## Usage

```sh
askdown questions.md            # open the window, wait for the answers
askdown --list questions.md     # print the questions found, no window
askdown --no-write questions.md # answers on stdout only
askdown --timeout 600 questions.md
```

| Exit code | Meaning |
| --- | --- |
| 0 | Answered. JSON on stdout. |
| 1 | Error (file not found, no questions, …). |
| 2 | Cancelled: closed without submitting. |
| 3 | Timed out (`--timeout`). |

## Writing the questions

A question is a fenced code block with the info string `askdown`, holding YAML.
Everything else in the file is shown as Markdown around the questions.

````markdown
Which environment should I deploy to? See the [design doc](https://example.com).

```askdown
id: target
type: choice
required: true
options:
  - staging
  - { label: Production, value: production }
```
````

| Field | Used by | Meaning |
| --- | --- | --- |
| `type` | all | `choice`, `text`, `media-choice`, `point`, `draw`, `color`, `range` |
| `id` | all | Key of the answer. Defaults to `q1`, `q2`, … |
| `question` | all | The question, in Markdown. |
| `required` | all | Submit is refused until it is answered. |
| `multiple` | `choice`, `media-choice` | Allow more than one. |
| `other` | `choice` | Add a free-text field. |
| `options` | `choice`, `media-choice` | Strings, or `{ label, value, src, description }`. |
| `src` | `point`, `draw`, `range` | Image / video / audio, relative to the Markdown file. |
| `placeholder`, `multiline` | `text` | |
| `default` | all | Pre-filled value. |

Unknown fields are an error, so a typo does not silently change a question.

### Answers

On stdout, one JSON object keyed by question id:

```json
{
  "features": { "selected": ["new-ranking"], "other": "dark mode" },
  "target": { "selected": ["staging"] },
  "note": "Ship it"
}
```

In the file, right after each question:

````markdown
<!-- askdown:answer -->
```yaml
selected:
- staging
```
<!-- /askdown:answer -->
````

Running `askdown` again on the same file replaces these blocks and starts the
window from the previous answers.

## Layout

```
crates/askdown-core   parsing, answer write-back, Markdown rendering (no IO)
crates/askdown-cli    the askdown binary: command line + Tauri window (ui/)
examples/             sample question files
```

## Development

```sh
cargo test
cargo run -p askdown -- examples/questions.md
```

## License

MIT
