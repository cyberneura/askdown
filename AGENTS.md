# askdown 開発メモ

AI エージェントが Markdown に書いた質問を GUI (Tauri) で人間に見せ、回答を stdout (JSON) と
同じ Markdown に書き戻すツール。AskUserQuestion の拡張版で、それを持たない Codex などで使う。
仕様と使い方は README.md を参照。ここには開発上の判断と注意点だけを書く。

構成は姉妹プロジェクト [runandlog](https://github.com/cyberneura/runandlog) に揃えている。
迷ったら runandlog の実装・AGENTS.md を参照する。

## 言語の方針

README・コードコメント・UI 文字列・エラーメッセージはすべて英語で書く。
この AGENTS.md (CLAUDE.md はそのシンボリックリンク) だけは日本語。

## 設計の要点

- **UI は GUI だけ。** CLI は起動オプションを受けるためだけにあり、TUI や対話プロンプトで
  選ばせる機能は作らない (四柳の指示)。`--list` は確認用の表示で、入力 UI ではない。
- **コアは IO を持たない。** `askdown-core` はパース (`parse`)・質問の型 (`question`)・
  回答の書き戻し (`answer`)・Markdown → HTML (`markdown`) だけ。ファイルの読み書きは
  `askdown` クレートの `session` が行う。
- **呼び出しはブロッキング。** エージェントは `askdown file.md` を実行して終了を待ち、
  stdout の JSON を読む。終了コードで回答 (0) / エラー (1) / キャンセル (2) / タイムアウト (3)
  を区別する。
- **質問は ```` ```askdown ```` フェンスの中の YAML。** `Question` は `deny_unknown_fields`。
  `mutliple` のような typo を黙って無視すると、単一選択に化けて気付けないため。
  読めないブロックは捨てずに、エラー内容をウィンドウに出す。
- **書き戻しは範囲置換だけ。** `parse` が質問ブロックと回答ブロックのバイト範囲を持ち、
  `write_back` がそこだけを置き換える。Markdown を再構築しないので原文の書式が残る。
- **回答は `<!-- askdown:answer -->` 〜 `<!-- /askdown:answer -->` で挟む。** 中身は YAML
  (人が読むため)。終端マーカーが無い開始マーカーは回答扱いしない (推測で範囲を決めると
  ユーザーの文章を巻き込むため)。
- **保存時にファイルを読み直す** (`Session::save`)。ウィンドウを開いている間にエージェントや
  ユーザーがファイルを編集していても、それを上書きで消さないため。回答は id で対応付ける。
- **Markdown は Rust 側で HTML にしてサニタイズする** (pulldown-cmark + ammonia)。
  書くのは AI で、表示先は Tauri の invoke を呼べる webview なので、script やイベント
  ハンドラを通さない。`<video>` / `<audio>` は質問の用途上許可している。
  フロントエンドはビルド工程を持たない素の JS (runandlog と同じ)。

## 回答の形 (JSON)

| type | 形 |
| --- | --- |
| `choice` | `{ "selected": [value...], "other"?: string }` (単一選択でも配列) |
| `text` | string |
| その他 | 未実装。実装時にここと README に書く |

未回答の質問はキーごと出さない。

## 実装状況

できている:

- workspace 構成 (`askdown-core` / `askdown-cli`)、clap の起動オプション
  (`--list` / `--no-write` / `--timeout` / `--title`)
- `choice` (単一 / 複数 / other 自由入力)、`text` (1 行 / 複数行) の GUI と書き戻し
- 前回の回答をウィンドウの初期値にする、`required` のチェック

未実装 (タスク CYBERNEURA-DEV-1057 の範囲):

- **ローカルファイルの表示。** Markdown 内の相対パスの画像・動画、`src` は今は表示できない。
  Markdown ファイルのディレクトリ以下だけを返すカスタム URI スキーム (または asset protocol +
  scope) を作り、相対パスをそれに書き換える。ディレクトリ外を読ませないこと。
- **リンクを外部ブラウザで開く** (tauri-plugin-opener。今はクリックを握りつぶしている)。
  http(s) だけ許可する。
- `media-choice`: 画像・動画の選択肢 (単一 / 複数)
- `point`: 画像・動画上の座標指定 (動画は再生位置も)。座標は元画像のピクセル基準 + 比率
- `draw`: フリーハンド描画 (背景画像可)。PNG を Markdown の隣に保存し、回答にはそのパス
- `color`: 色指定
- `range`: 音声・動画の区間指定 (ytyng-blog のサビ選択のような UI)。開始・終了秒
- アイコン (今は runandlog のものを仮置き)、CI (`.github/workflows/test.yml` のみ)、
  リリース (runandlog の release.yml / scripts/release.sh / Homebrew tap を参考に)、
  サードパーティライセンス表示

## 検証

```sh
cargo test
cargo clippy --all-targets
cargo run -p askdown -- --list examples/questions.md
cargo run -p askdown -- examples/questions.md   # ウィンドウが開く
```

examples の Markdown を直接答えると書き戻しで変更されるので、/tmp にコピーして試す。
