# askdown

Ask a person questions from a Markdown file, in a desktop window.

An AI agent writes its questions into a Markdown file and runs `askdown` on it.
A window opens showing the Markdown (links, images, video) with an input for each
question. When the user presses **Submit**, the answers are printed to stdout as
JSON and written back into the file under each question. The command blocks until
then, so it works as an `AskUserQuestion` for agents that do not have one, such as Codex.

Questions can be multiple choice (text or pictures), free text, a point on an
image or a video frame, a freehand drawing, a colour, or a span of an audio or
video file.

## Install

On macOS (Apple Silicon), through Homebrew:

```sh
brew install --cask cyberneura/tap/askdown
```

Or from source, on macOS or Linux:

```sh
cargo install --path crates/askdown-cli
```

Each installs the `askdown` binary. It is a desktop window, so it needs the
system webview: WebKit on macOS, and on Linux `libwebkit2gtk-4.1` (plus GTK 3),
both to build and to run. The Linux archive on the
[releases page](https://github.com/cyberneura/askdown/releases) is built on
Ubuntu and expects that library to be installed.

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
| `default` | all but `draw` | Pre-filled value, in the shape of the answer. `choice` and `media-choice` also take a value or a list of values. |
| `multiple` | `choice`, `media-choice`, `point` | Allow more than one. |
| `other` | `choice` | Add a free-text field. |
| `options` | `choice`, `media-choice` | Strings, or `{ label, value, src, description }`. For `media-choice`, a bare string is the file to show. |
| `src` | `point`, `draw`, `range` | Image / video / audio: a file path relative to the Markdown file (not URL-encoded). |
| `placeholder`, `multiline` | `text` | |
| `width`, `height` | `draw` | Size of the blank sheet when there is no `src` (default 800 × 600). |

Unknown fields are an error, so a typo does not silently change a question.

### Pictures, video, audio and links

Relative paths in the Markdown and in `src` are resolved against the Markdown
file. Only files in that file's directory (and below) are served; anything else
is refused. Links to `http(s)` URLs open in the default browser.

In the Markdown, a destination is a URL, as CommonMark has it: write `%20` for
a space and `%23` for a `#` in a file name. In the YAML (`src`), it is a plain
file path, written as it is.

````markdown
![Current layout](shots/layout.png)

<video src="clips/take-3.mp4" controls></video>
````

### Answers

On stdout, one JSON object keyed by question id. Unanswered questions are left out.

| `type` | Answer |
| --- | --- |
| `choice`, `media-choice` | `{ "selected": ["value", …], "other"?: "text" }` (an array even for a single choice) |
| `text` | `"text"` |
| `color` | `"#rrggbb"` |
| `point` | `{ "width", "height", "points": [{ "x", "y", "rx", "ry", "time"? }] }` — `x`/`y` in pixels of the original, `rx`/`ry` as a fraction of its size, `time` the video position in seconds |
| `draw` | `{ "path": "questions.sketch.png", "width", "height" }` — the picture, saved next to the Markdown as `<file>.<id>.png` (characters other than letters, digits, `-` and `_` in the id are replaced and a short hash is added) |
| `range` | `{ "start", "end", "duration" }` in seconds |

```json
{
  "features": { "selected": ["new-ranking"], "other": "dark mode" },
  "target": { "selected": ["staging"] },
  "focus": { "width": 1280, "height": 720, "points": [{ "x": 640, "y": 360, "rx": 0.5, "ry": 0.5 }] },
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
window from the previous answers. A `draw` question then shows the previous
picture as its background, and keeps it as the answer unless something is drawn.

`--no-write` leaves the Markdown untouched. The picture of a `draw` question is
still saved, since that file is the answer.

See [`examples/`](examples/) for a file with every kind of question.

## Layout

```
crates/askdown-core   parsing, answer write-back, Markdown rendering (no IO)
crates/askdown-cli    the askdown binary: command line + Tauri window (ui/)
examples/             sample question files
```

## Development

```sh
cargo test
cargo run -p askdown -- examples/media.md
```

Releases follow the version in `Cargo.toml`: `scripts/release.sh [patch|minor|major]`
bumps it and pushes `main`, and the release workflow builds, signs and publishes
that version. The Homebrew tap picks the release up by itself.

## License

MIT. The libraries compiled into askdown have their own licenses, listed in
[THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt) and printed by
`askdown --license`. The file is generated; after adding or upgrading a
dependency, generate it again and commit the result:

```sh
cargo install cargo-about --locked --features cli   # once
scripts/generate-third-party-notices.sh
```

`cargo test` fails when a direct dependency is missing from the file or a listed
crate is no longer in `Cargo.lock`.
