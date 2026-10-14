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
  `askdown` クレートの `session` (Markdown と、その隣への書き込み) と `media` (読み出し) が行う。
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

## ローカルファイルの表示 (`media.rs`)

- **カスタム URI スキーム `askdown`** を `register_uri_scheme_protocol` で登録し、
  Markdown ファイルのディレクトリ以下だけを返す。URL は macOS / Linux が
  `askdown://localhost/<相対パス>`、Windows が `http://askdown.localhost/<相対パス>`
  (`media::base_url`)。Tauri の asset protocol + scope でも書けるが、URL に絶対パスを
  埋める必要があり、ディレクトリ境界の判定も自前で持ちたかったので自作した。
- **境界は `resolve` の 1 箇所で守る。** `..` や絶対パスを含む要求は 403、`canonicalize` した
  結果がディレクトリの外に出る (symlink) 場合も 403、無いファイルは 404。Markdown の
  相対 URL を書き換える側 (`markdown::media_url`) は正規化しない (`..` は webview が URL を
  解釈する時点で解決されるので、サーバーにはまず届かない)。判定はサーバーの 1 箇所だけ。
- **守るのは「AI が書いた Markdown がディレクトリの外を指す」ことだけ。** 同じユーザーが
  同じディレクトリで同時に symlink を差し替える TOCTOU や、外のファイルへの hard link は
  ローカルツールの脅威モデル外 (そのユーザーは既にそのファイルを直接読み書きできる)。
  Codex レビューで指摘されたが対応しない。
- **`Range` ヘッダ (単一範囲) に対応する。** WKWebView は動画のシークで範囲要求を出す。
  複数範囲はメディアプレイヤーが出さないので未対応。
- **レスポンスに `Access-Control-Allow-Origin: *` を付ける。** ウィンドウのオリジン
  (`tauri://localhost`) とは別オリジンなので、これが無いと `draw` の背景画像を描いた
  canvas が tainted になり `toDataURL` が失敗する。フロントは `img.crossOrigin = "anonymous"`
  で読む。実機で PNG の保存まで確認済み。
- **Markdown 側の書き換えは ammonia の `UrlRelative::Custom`** (`markdown::Rewrite`)。
  `href` / `src` / `<video poster>` に効く。質問の `src` はフロントが `DocumentView.media_base` を
  前置する (`mediaUrl`。YAML の `src` はファイルパスなのでセグメントごとに
  `encodeURIComponent` する。`#` や `%` を含む名前をそのまま通すため)。CSP の `img-src` /
  `media-src` に両方の URL 形を入れてある。
- **リンクは `open_link` コマンド → `open` crate** で http(s) だけ開く。Markdown は AI が
  書くので `file:` や独自スキームは通さない。tauri-plugin-opener は同じ `open` を
  permission 機構付きで包んだものなので、自前コマンド 1 つで足りる。
- **動画は `loadedmetadata` で `currentTime = 0.001` にする** (`showFirstFrame`)。WebKit は
  再生かシークをするまで 1 フレームも描かないので、クリックする対象が真っ黒になる。

## 質問の種類とフロント (`ui/app.js`)

renderer は「`collect` を返す」か「`{ collect, drawing }` を返す」。`collect` は同期で、
回答 / `null` (未回答) / 例外 (不正な入力。メッセージをステータス行に出す) のいずれか。
`drawing` は保存する PNG (data URL) を返し、`submit` の `drawings` に載せる。

- `media-choice`: 画像はクリックで選択、動画・音声はコントロールがあるのでキャプションで
  選ぶ (カード全体を `<label>` にすると再生ボタンで選択が切り替わる)。
- `point`: 動画は native controls を出さず、自前の再生ボタン + シークバー (`videoControls`)。
  controls を出すとフレームの上の overlay がコントロールを塞ぐか、コントロールのクリックが
  点になる。マーカーは比率 (`rx` / `ry`) で置くので表示サイズに依らず前回回答を復元できる。
- `draw`: ストロークを配列で持ち、Undo は配列から外して描き直す。前回回答がある時は
  その PNG (背景込み) を背景にして上書きし、何も描かなければ前回回答をそのまま返して
  PNG は書き直さない。背景の読込中は描画も送信も受け付けない (canvas は仮サイズなので
  座標がずれ、背景無しで書き出してしまう)。読込に失敗したら白紙に切り替えず、描画不可の
  まま required なら送信を止め、任意なら未回答にする (背景無しの絵は別の質問への回答になる)。背景無しは白で塗る (透過 PNG は黒く見えるビューアが多い)。
  保存名は `<md の stem>.<id>.png` (`drawing_file_name`。id は YAML 由来なので英数字と `-_`
  以外は `_` に落とし、落とした場合は元の id のハッシュを付けて `a.b` と `a/b` が
  衝突しないようにする)。PNG は `submit` に同梱して Rust の `commit` が書く: 一時ファイルに
  書く → Markdown を保存 → rename、の順。Markdown の保存が失敗したら PNG は差し替えない
  (古い回答が未提出の絵を指すのを防ぐ)。保存先がディレクトリなら stage の時点で弾き
  (rename で置き換えられない唯一の現実的なケース)、それでも rename が失敗したら Markdown を
  保存前の内容に戻す。rename は既存の symlink / hard link を辿らず差し替える。`--no-write` でも PNG は書く (それが回答なので)。
- `color`: `<input type=color>` は必ず値を持つので、テキスト欄を正とし、空なら未回答。
- `src` が無い `point` のように入力を作れない質問は `unusable` で表示し、`required` なら
  submit を止める (そうしないと必須のまま未回答で正常終了できる)。
- `range`: ytyng-blog のサビ選択ウィジェットに倣い、native controls + 「Use current time」+
  ±0.1 / ±0.5 のナッジ + 区間再生 (`timeupdate` で `end` に達したら pause)。
  負数・`end <= start`・長さ超え・長さ未取得は例外で弾く。
- YAML の `src` と回答の `path` はファイルパスで URL ではない。`#` / `?` / `:` を含む名前も
  そのまま扱う (`mediaUrl` はスキーム判定をせず常にパスとしてエンコードし、`mediaKind` は
  末尾の拡張子だけを見る)。
- `point` の復元した値は送信時に検証する: `rx` / `ry` は 0..1、動画の `time` は 0..長さ
  (手で編集された回答や、差し替えで短くなった動画の外を指す点を出さない)。

## 回答の形 (JSON)

README の表が正。追加・変更したら README と `examples/media.md` も直す。

## 検証

```sh
cargo test
cargo clippy --all-targets
cargo run -p askdown -- --list examples/media.md
cargo run -p askdown -- examples/media.md   # ウィンドウが開く
```

examples の Markdown を直接答えると書き戻しで変更されるので、/tmp にコピーして試す
(`draw` は PNG も隣に書く)。

**実機 (macOS) の E2E は orca computer-use で回せる。** `orca computer get-app-state --app askdown`
で webview の DOM がアクセシビリティツリーに出るので、element-index で click できる。
落とし穴:

- `set-value` は `<input type=number>` に効かない (`value_not_settable`)。ボタン (Use current
  time / ナッジ) をクリックして値を入れる。
- `drag` は pointermove を配送しない (pointerdown / up だけ届くので点が 1 つ打たれる)。
  ストロークの検証は手で行うか、ブラウザにスタブで読ませる。
- 要素を追加する操作 (マーカーを置く等) で element-index がずれる。操作のたびに取り直す。
- 画面の下の方は `click` が自動でスクロールして見せる。座標クリックの前に screenshot を
  取り直し、その間にスクロールしない。

## 未実装

- アイコン (今は runandlog のものを仮置き。`scripts/generate-icon.py` を runandlog から
  持ってきて askdown の意匠にする)
- リリース (runandlog の `release.yml` / `scripts/release.sh` / Homebrew tap を参考に)、
  サードパーティライセンス表示 (`about.toml` / `generate-third-party-notices.sh`)
- Linux の実機確認 (CI では Ubuntu で clippy とテストを通しているだけ)。Windows は CI にも
  入っていない (`media::base_url` の `http://askdown.localhost/` 形は未検証)
