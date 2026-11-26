# askdown development notes

A tool that shows questions an AI agent wrote in Markdown to a human in a GUI (Tauri), and writes
the answers back to stdout (JSON) and to the same Markdown file. It is an extended version of
AskUserQuestion, meant for agents that do not have one, such as Codex.
See README.md for the specification and usage. This file records only development decisions and caveats.

The layout follows the sister project [runandlog](https://github.com/cyberneura/runandlog).
When in doubt, refer to runandlog's implementation and AGENTS.md.

## Language policy

Write everything in English: README, all other documentation including this AGENTS.md
(CLAUDE.md is a symlink to it), code comments, UI strings, and error messages.

## Design essentials

- **The UI is GUI only.** The CLI exists only to accept launch options; do not build features that
  let the user choose via a TUI or interactive prompts (Yotsuyanagi's instruction). `--list` is a
  display for checking, not an input UI.
- **The core has no IO.** `askdown-core` only does parsing (`parse`), question types (`question`),
  writing answers back (`answer`), and Markdown → HTML (`markdown`). File reading and writing is
  done by the `askdown` crate's `session` (writing to the Markdown file and next to it) and `media`
  (reading).
- **Invocation is blocking.** The agent runs `askdown file.md`, waits for it to exit, and reads
  the JSON on stdout. The exit code distinguishes answered (0) / error (1) / cancelled (2) /
  timed out (3).
- **A question is YAML inside a ```` ```askdown ```` fence.** `Question` uses `deny_unknown_fields`,
  because silently ignoring a typo such as `mutliple` would turn it into a single choice without
  anyone noticing.
  If even one block cannot be read, do not open the window; print the location and the error to
  stderr and exit 1 (`check_blocks` in `main.rs`; `--list` behaves the same. This is so the calling
  AI fixes it. Showing it in the window would not reach the AI, and the human looking at it cannot
  fix the file. Yotsuyanagi's instruction). The `error` display in `gui.rs` / `app.js` is not
  reachable from the CLI.
- **Writing back is range replacement only.** `parse` keeps the byte ranges of question blocks and
  answer blocks, and `write_back` replaces only those. The Markdown is not rebuilt, so the original
  formatting is preserved.
- **Answers are enclosed between `<!-- askdown:answer -->` and `<!-- /askdown:answer -->`.** The
  content is YAML (so people can read it). A start marker without an end marker is not treated as
  an answer (guessing the range would swallow the user's text).
- **The file is re-read on save** (`Session::save`), so that edits the agent or the user made to the
  file while the window was open are not overwritten and lost. Answers are matched by id.
- **Markdown is converted to HTML and sanitized on the Rust side** (pulldown-cmark + ammonia).
  The author is an AI and the destination is a webview that can call Tauri's invoke, so scripts and
  event handlers are not let through. `<video>` / `<audio>` are allowed because questions need them.
  The frontend is plain JS with no build step (same as runandlog).

## Code blocks (`wrapCodeBlock`, `ui/app.js`)

`trustedHtml` wraps each Markdown `<pre>` in `.code-block` and puts a copy button next to it
(putting the button inside the `<pre>` would mix it into what gets copied or selected). The
background is black in both light and dark mode, and long lines wrap. Copying calls
`navigator.clipboard` directly from the click with no await (same as runandlog; WebKit only allows
writes during a user action. Do not add a plugin). The trailing newline is removed (pulldown-cmark
always adds one, and pasting it into a terminal would execute the command).

## Tables (`wrapTable`, `ui/app.js`)

`trustedHtml` wraps each Markdown `<table>` in `.table-scroll`. A table wider than the window
scrolls horizontally only inside this wrapper (the surrounding text and questions do not move).
Cell background is `--card`, headers use `--table-head`, and headers are left-aligned.

## Displaying local files (`media.rs`)

- **A custom URI scheme `askdown`** is registered with `register_uri_scheme_protocol` and serves
  only files under the Markdown file's directory. URLs are `askdown://localhost/<relative path>` on
  macOS / Linux and `http://askdown.localhost/<relative path>` on Windows (`media::base_url`).
  Tauri's asset protocol + scope could do this too, but it requires embedding absolute paths in the
  URL, and we wanted to own the directory-boundary check, so we built our own.
- **The boundary is enforced in one place, `resolve`.** Requests containing `..` or an absolute
  path get 403; if the `canonicalize`d result leaves the directory (symlink), also 403; a missing
  file gets 404. The side that rewrites relative URLs in Markdown (`markdown::media_url`) does not
  normalize (`..` is resolved when the webview interprets the URL, so it basically never reaches
  the server). The check lives only in the server.
- **The only thing defended against is "Markdown written by the AI pointing outside the
  directory."** A TOCTOU where the same user swaps a symlink in the same directory concurrently, or
  a hard link to a file outside, is outside the threat model of a local tool (that user can already
  read and write that file directly). This was pointed out in a Codex review but will not be
  addressed.
- **`Range` headers (single range) are supported.** WKWebView issues range requests when seeking
  in videos. Multiple ranges are not supported because media players do not issue them.
- **Responses carry `Access-Control-Allow-Origin: *`.** The window's origin (`tauri://localhost`)
  is a different origin, so without this the canvas on which a `draw` background image is drawn
  becomes tainted and `toDataURL` fails. The frontend loads with `img.crossOrigin = "anonymous"`.
  Verified on a real machine all the way to saving the PNG.
- **Rewriting on the Markdown side uses ammonia's `UrlRelative::Custom`** (`markdown::Rewrite`).
  It applies to `href` / `src` / `<video poster>`. For a question's `src`, the frontend prepends
  `DocumentView.media_base` (`mediaUrl`. The YAML `src` is a file path, so each segment is
  `encodeURIComponent`-ed, so names containing `#` or `%` pass through as is). Both URL forms are
  included in the CSP's `img-src` / `media-src`.
- **Links open via the `open_link` command → the `open` crate**, http(s) only. Markdown is written
  by an AI, so `file:` and custom schemes are not let through. tauri-plugin-opener is the same
  `open` wrapped with a permission mechanism, so a single command of our own is enough.
- **Videos set `currentTime = 0.001` on `loadedmetadata`** (`showFirstFrame`). WebKit does not draw
  a single frame until playback or a seek, so the thing to click on would be pitch black.

## Question types and the frontend (`ui/app.js`)

A renderer returns either "`collect`" or "`{ collect, drawing }`". `collect` is synchronous and
returns an answer, `null` (unanswered), or throws (invalid input; the message is shown in the status
line). `drawing` returns the PNG (data URL) to save, which is put into `drawings` of `submit`.

- `media-choice`: images are selected by clicking; videos and audio have controls, so they are
  selected via the caption (making the whole card a `<label>` would toggle selection when the play
  button is pressed).
- `point`: videos do not show native controls; they use our own play button + seek bar
  (`videoControls`). With controls shown, either the overlay on top of the frame blocks the
  controls, or clicks on the controls become points. Markers are placed by ratio (`rx` / `ry`), so
  the previous answer can be restored regardless of display size.
- `draw`: strokes are kept in an array, and Undo removes one from the array and redraws. When there
  is a previous answer, its PNG (including background) is used as the background and drawn over; if
  nothing is drawn, the previous answer is returned as is. With no background, it is filled white
  (many viewers show transparent PNGs as black). While the background is loading, neither drawing
  nor submitting is accepted (the canvas has a provisional size, so coordinates would be off). If
  loading fails, it does not switch to a blank canvas; if required, submission is blocked, and if
  optional, it becomes unanswered (a drawing without the background would be an answer to a
  different question). The PNG is bundled into `drawings` of `submit` and Rust writes it (see
  "Saving drawings" below).
- `color`: `<input type=color>` always has a value, so the text field is the source of truth, and
  empty means unanswered.
- A question for which no input can be built, such as a `point` without `src`, is shown as
  `unusable`, and if `required`, submit is blocked (otherwise it could exit successfully with a
  required question unanswered).
- `range`: modeled after ytyng-blog's chorus-selection widget: native controls + "Use current time"
  + ±0.1 / ±0.5 nudges + range playback (pause when `timeupdate` reaches `end`).
  Negative numbers, `end <= start`, exceeding the duration, and an unknown duration are rejected by
  throwing.
- The YAML `src` and the answer's `path` are file paths, not URLs. Names containing `#` / `?` / `:`
  are handled as is (`mediaUrl` does no scheme detection and always encodes as a path, and
  `mediaKind` looks only at the trailing extension).
- Restored `point` values are validated on submit: `rx` / `ry` in 0..1, and a video's `time` in
  0..duration (so that hand-edited answers, or points outside a video that got shorter after being
  replaced, are not emitted).

## Saving drawings (`gui::commit`, `session::stage_file` / `commit_file`)

The Markdown is the authoritative record, so a picture is replaced only "after the answer pointing
to that picture has been written." The order is: write to a temporary file (`stage_file`, taking an
unused short name with `create_new`) → save the Markdown (`Session::save`, which returns the content
just before saving) → move the existing file aside, then rename (`commit_file` → `Replaced`). On
failure, the Markdown is restored to its pre-save state with `restore`, and already-replaced
pictures are put back with `undo`. If an external edit happened after saving and the Markdown cannot
be restored, the written answer remains, so the pictures are not put back either, and the error says
so. When everything succeeds, the moved-aside files are deleted with `finish`. If the destination is
a directory, it is rejected at stage time (the only realistic case where rename cannot replace it).
rename replaces existing symlinks / hard links without following them. PNGs are written even with
`--no-write` (because they are the answer).

The file name is `drawing_file_name`: `<md stem>.<id>.png`. In the id, anything other than lowercase
alphanumerics and `-_` is reduced to `_`, and if it changed (including when it contains uppercase,
because it would collide on a case-insensitive FS), a 16-digit FNV hash of the original id is
appended after a `.`. For names over 200 bytes, the stem / id are truncated to 64 bytes and a hash of
the whole is appended.

## Answer shape (JSON)

The table in the README is authoritative. When adding or changing anything, also update the README
and `examples/media.md`.

## CI and releases

`.github/workflows/test.yml` is the PR check, and it is also called from `release.yml` via `uses:`
to act as the release gatekeeper (a called job joins the caller's run, so `needs` works).
It runs clippy (`-D warnings`) and tests on both macOS and Ubuntu. On Ubuntu, webkit2gtk 4.1 is
installed with apt (askdown is GUI-only and has no feature split).

**Changing the version in `Cargo.toml` on main makes a release.** `release.yml` runs on every push
to main and decides based on whether a Release for that version is already published (only a 404 is
read as "unpublished"). It is safe to run any number of times, and a failed release continues when
you fix the cause and push.
`scripts/release.sh [patch|minor|major]` is a thin script that bumps the version in 3 places (the
workspace `version`, the `version` of the `askdown-core` dependency, and `tauri.conf.json`) +
`Cargo.lock` and pushes.

- macOS is arm64 only, with Developer ID signing + notarization (secrets `APPLE_*` are already set).
  A bare binary cannot be stapled, so Gatekeeper fetches the ticket online.
- Linux is x86_64, a binary dynamically linked against webkit2gtk 4.1 (required at runtime;
  documented in the README).
- The Release is created as a draft and published after counting the assets (2).
- Do not touch the Homebrew cask in this repository. A workflow in `cyberneura/homebrew-tap` checks
  the latest Release hourly and updates `Casks/askdown.rb` (the cask's `binary` path matches the
  directory name inside the archive, `askdown-v<version>-aarch64-apple-darwin/askdown`).
- crates.io is enabled with the repository variable `PUBLISH_CRATES=true` and the secret
  `CARGO_REGISTRY_TOKEN` (not set). "A push continues it" holds only up to the GitHub Release; if
  only the `crates` job fails after the Release is published, `plan` judges it "already published"
  and does nothing. In that case, run `crates` with "Re-run failed jobs" in Actions. The `askdown`
  crate is published with `--no-verify` (tauri-build writes `gen/schemas/` into the source, so the
  verification build is rejected).

## License notices

`THIRD-PARTY-NOTICES.txt` is generated by `scripts/generate-third-party-notices.sh` (cargo-about,
`about.toml`), and `notices.rs` `include_str!`s it through a symlink from `crates/askdown-cli/`
(`askdown --license`). `tests/third_party_notices.rs` checks it against Cargo.lock, so when you add
or bump dependencies, regenerate it and commit.

## Verification

```sh
cargo test
cargo clippy --all-targets
cargo run -p askdown -- --list examples/media.md
cargo run -p askdown -- examples/media.md   # opens a window
```

Answering the examples' Markdown directly modifies them through write-back, so copy them to /tmp to
try them (`draw` also writes a PNG next to it).

**E2E on a real machine (macOS) can be run with orca computer-use.**
`orca computer get-app-state --app askdown` exposes the webview's DOM in the accessibility tree, so
you can click by element-index.
Pitfalls:

- `set-value` does not work on `<input type=number>` (`value_not_settable`). Click the buttons (Use
  current time / nudges) to enter values.
- `drag` does not deliver pointermove (only pointerdown / up arrive, so a single point is placed).
  Verify strokes by hand, or have a browser load it with stubs.
- Operations that add elements (placing a marker, etc.) shift the element-index. Re-fetch it after
  every operation.
- For the lower part of the screen, `click` scrolls automatically to show it. Re-take the screenshot
  before a coordinate click, and do not scroll in between.

## App icon

A white question mark on a purple gradient. **Do not draw it by hand; generate it with
`scripts/generate-icon.py`** (runs with the standard library only. This repository has no image
toolchain, and adding one for a single asset is not worth it). The script writes 3 files:
`resources/app-icons/mac-original.png`, its copy `crates/askdown-cli/icons/icon.png`
(`bundle.icon` in `tauri.conf.json`), and `resources/app-icons/layout.json`. The glyph rectangle in
layout.json is computed from the drawn shapes, so it never drifts from the original image the way a
hand measurement could. The same source produces the same pixels, but not identical bytes
(differences in zlib implementations can change the compressed output, and libm differences can
change rounding). Check whether a regeneration is correct not with `cmp` or git diff but with
`python3 scripts/generate-icon.py --check` (compares pixels with a per-channel tolerance of ±1;
`.github/workflows/macos-icons.yml` does the same).

The glyph is drawn with signed distance functions (arc + capsule stem + dot), and the rounded-square
body uses the macOS template dimensions as constants (an 824 body on a 1024 canvas, 22.5% corner
radius). The glyph is white, and the background is a color with a low red component. The per-size
image generation (`prepare-macos-icons.swift`) recovers the glyph's alpha from "how much higher each
pixel's red component is than the background's," so a colored glyph or a strongly red background
breaks it.

When you change the glyph or colors, fix the script and regenerate, then run
`bash scripts/generate-macos-icons.sh` (macOS + Xcode) to propagate to per-size PNGs, ICNS, and
Assets.car. Dimension details and verification commands are in `resources/app-icons/README.md`.
At 16pt / 32pt @1x the background reaches the edge of the canvas and the glyph fits within the
central 80%. Larger sizes keep a 10% margin on the outside. Do not rebuild the generated ICNS with
`tauri icon` / `iconutil` (that loses the ARGB storage of small sizes and the corresponding
Assets.car / CFBundleIconName registration). Native rendering is verified by `macos-icons.yml`.

What we distribute is a bare executable, so the GUI embeds the ICNS (`include_bytes!` in `gui.rs`)
and sets it as the NSApplication icon at launch. Bundle registration only affects an arbitrary .app
build; a bare binary cannot be given a system-settings icon.

## Not implemented

- Verification on a real Linux machine (CI only runs clippy and tests on Ubuntu). Windows is not even
  in CI (the `http://askdown.localhost/` form of `media::base_url` is unverified)
