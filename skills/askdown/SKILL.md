---
name: askdown
description: Ask the user questions through askdown, a desktop window driven by a Markdown file — single or multiple choice (text or pictures), free text, a point on an image or video frame, a freehand drawing, a colour, or a span of an audio or video file. Use when an agent without an AskUserQuestion tool (Codex and others) needs a decision or input from the person, when the question involves media the person has to look at or listen to, or when "askdown" is mentioned. Not for questions a plain chat reply can carry.
---

# askdown: ask the user through a window

[askdown](https://github.com/cyberneura/askdown) opens a desktop window showing a Markdown
file with an input under each question, waits for the person to press **Submit**, prints
the answers as JSON on stdout and writes them back into the same file. The command blocks
until then, so it is an `AskUserQuestion` for agents that have none, and it can show
pictures, video and audio that a chat reply cannot.

Prerequisite: the `askdown` binary (`brew install --cask cyberneura/tap/askdown` on macOS,
or `cargo install --path crates/askdown-cli` from the repository). The window needs a
desktop session; it does not work over a plain SSH shell.

## The loop

1. Write the questions into a Markdown file. Put it next to any media it refers to.
2. Check it: `askdown --list questions.md` prints the questions found (no window) and shows
   a YAML error inline if a block cannot be read.
3. Run `askdown questions.md` and wait. Give a timeout when the run must not hang forever:
   `askdown --timeout 900 questions.md`.
4. Read the exit code, then stdout.

| Exit | Meaning | What to do |
|---|---|---|
| 0 | Answered; stdout holds one JSON object keyed by question id | Use the answers |
| 1 | Error (file missing, no question blocks, write-back failed); stderr says why | Fix the file |
| 2 | Cancelled: the person closed the window without submitting | Do not retry blindly; ask in chat what they want |
| 3 | Timed out (`--timeout`) | Same as cancelled |

The answers are also written into the file under each question, between
`<!-- askdown:answer -->` and `<!-- /askdown:answer -->`, as YAML. Running askdown again on
the same file starts from those answers and replaces the blocks, so a file can be reused for
a follow-up round. `--no-write` reports on stdout only. Questions left unanswered do not
appear in the JSON at all.

## Writing a question

A question is a fenced code block with the info string `askdown`, holding YAML. Everything
else in the file is ordinary Markdown shown around the questions: use it for context,
links (they open in the browser) and pictures.

````markdown
# Deploy checklist

See the [design doc](https://example.com/design). Current layout:

![layout](shots/layout.png)

```askdown
id: target
type: choice
question: Which environment should I deploy to?
required: true
options:
  - staging
  - { label: Production, value: production, description: Needs a release note }
```

```askdown
id: note
type: text
question: Anything to add to the release note?
multiline: true
```
````

| Field | Used by | Meaning |
|---|---|---|
| `type` | all | `choice`, `text`, `media-choice`, `point`, `draw`, `color`, `range` |
| `id` | all | Key in the JSON. Defaults to `q1`, `q2`, … — set one you can read back |
| `question` | all | The question, in Markdown |
| `required` | all | Submit is refused until it is answered |
| `default` | all but `draw` | Pre-filled value, in the answer's shape (`choice` also takes a value or a list) |
| `multiple` | `choice`, `media-choice`, `point` | Allow more than one |
| `other` | `choice` | Add a free-text field; it comes back as `other` |
| `options` | `choice`, `media-choice` | Strings, or `{ label, value, src, description }` |
| `src` | `point`, `draw`, `range` | Image / video / audio, a path relative to the Markdown file |
| `placeholder`, `multiline` | `text` | |
| `width`, `height` | `draw` | Blank sheet size when there is no `src` (default 800 × 600) |

Unknown fields are an error (a typo like `mutliple` is refused rather than ignored), so
always run `--list` after writing.

Paths: in the Markdown prose a destination is a URL (`shots/my%20layout.png`); in YAML
`src` it is a plain path (`shots/my layout.png`). Only files in the Markdown file's
directory and below are served, so keep the media next to the file.

## What each kind returns

| `type` | Answer |
|---|---|
| `choice`, `media-choice` | `{ "selected": ["value", …], "other"?: "text" }` — an array even for a single choice |
| `text` | `"text"` |
| `color` | `"#rrggbb"` |
| `point` | `{ "width", "height", "points": [{ "x", "y", "rx", "ry", "time"? }] }` — `x`/`y` in pixels of the original, `rx`/`ry` as a fraction 0..1, `time` the video position in seconds |
| `draw` | `{ "path": "questions.sketch.png", "width", "height" }` — the picture, saved next to the Markdown as `<file>.<id>.png` (written even with `--no-write`) |
| `range` | `{ "start", "end", "duration" }` in seconds |

Picking the kind:

- **A decision between named things**: `choice`. Add `other: true` when the list may be
  incomplete; put the recommended option first and say why in `description`.
- **A decision between pictures or clips** (thumbnails, takes, variants): `media-choice`
  with one `src` per option. A bare string option is the file itself.
- **"Where?" on a picture or a frame**: `point`. For a video the person seeks to the frame
  first; the answer carries `time`. Use `rx`/`ry` when you need a position that survives
  resizing, `x`/`y` for pixels of the file as it is.
- **"Draw it for me"** (a crop, a region, a sketch): `draw` with `src` for a background, or
  `width`/`height` for a blank sheet. Read the PNG at `path` afterwards.
- **"Which part of the audio or video?"** (a chorus, a cut): `range`.
- **A colour**: `color`.

## Reading the answers

```sh
askdown --timeout 900 questions.md > answers.json; status=$?
```

Then branch on `$status` before parsing `answers.json`. A required question that is
unanswered cannot happen (Submit refuses it); an optional one is simply absent from the
object, so test for the key rather than for `null`.

## Keep the person's time in mind

- One file per round, several questions in it: the window shows them all at once. Do not
  open a window for every single question.
- Say in the prose what the answer will be used for; the person sees nothing but the file.
- Prefer `choice` with an `other` field over free text when you can enumerate the answers.
- `--title "..."` sets the window title; the default is the file name.
