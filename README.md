# rust-axum-image-bbs

昔 PHP で作った画像掲示板を、Rust + axum で作り直した学習用プロジェクトです。

- 投稿一覧（新しい順、1ページ20件のページ送り）
- 画像つきの投稿（jpg / gif / png、5MB まで。形式はファイルの中身で判定）
- 編集（タイトル・本文の変更、画像の差し替え・画像だけの削除）
- 削除（画像ファイルも一緒に消す）

ログインや管理画面はありません。実装の過程は Zenn の記事として段階的に公開する想定で、
ソースコードには Rust 初心者向けの日本語コメント（所有権・借用・`Result`・`?`・`async/.await` など）を詳しく書いています。

使っている主なクレート: axum 0.7 / sqlx 0.8（SQLite）/ Askama 0.12 / tower-http 0.5 / tracing

## 動作要件

- Rust（stable）。動作確認は Rust 1.97。`Cargo.lock` の依存クレートは Rust 1.89 以上を要求します
- SQLite を別に入れる必要はありません（sqlx がビルド時に組み込みます）
- 任意: Docker（`docker compose` が使えるもの）、[just](https://github.com/casey/just)、curl

## ローカルで動かす

```sh
cp .env.example .env
cargo run
```

ブラウザで http://localhost:3000/ を開きます。curl なら次のとおりです。

```sh
curl -i http://localhost:3000/
```

- DB ファイル（`data/app.db`）と画像の保存先（`uploads/`）は起動時に自動で作られます
- テーブルを作るマイグレーション（`migrations/`）も起動時に自動で実行されます。手で SQL を流す必要はありません
- `.env` を作らなくても、下の表の既定値で起動します

## 環境変数

`.env` に書くか、`PORT=8080 cargo run` のようにコマンドの前に付けて渡します（コマンドの前に付けたほうが優先）。

| 変数 | 既定値 | 説明 |
|---|---|---|
| `DATABASE_URL` | `sqlite://data/app.db` | SQLite の接続先。ファイルが無ければ作る |
| `UPLOAD_DIR` | `uploads` | 画像の保存先。`{投稿id}.{拡張子}` の名前で保存し、`/uploads/` で配信する |
| `PORT` | `3000` | 待ち受けるポート |
| `RUST_LOG` | `info` | ログの詳しさ（例: `debug`、`info,rust_axum_image_bbs=debug`） |
| `LOG_FORMAT` | （なし） | `json` にすると1行1つの JSON でログを出す |

## ログ

既定では、リクエスト1件につき1行（メソッド・URL・ステータス・所要時間）が出ます。
投稿の作成・更新・削除では、どの投稿かが分かるよう `post_id` が付きます。タイトルや本文はログに出しません。

```text
INFO request{method=POST uri=/posts version=HTTP/1.1}:create{post_id=1}: rust_axum_image_bbs::handlers::posts: post created
INFO request{method=POST uri=/posts version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=16 ms status=303
```

```sh
RUST_LOG=debug cargo run      # もっと詳しく
LOG_FORMAT=json cargo run     # 1行1つの JSON（ログ集約ツールに読ませるとき）
```

## テスト

```sh
cargo test
```

- `src/` の中のユニットテストと、`tests/` の統合テストが走ります
- 統合テストはサーバを起動せず、ルーターに直接 HTTP リクエストを投げます。DB はテストごとの in-memory SQLite、画像の保存先は一時ディレクトリです
- 受け入れ基準のテストケース（TC-...）とテストの対応表: [implements/TASK-0018/coverage.md](docs/tumigidoc/rust-axum-image-bbs/implements/TASK-0018/coverage.md)

本物のサーバを起動して curl で一通り確かめるスクリプトもあります（一時的な DB と保存先を使うので、手元のデータには触りません）。

```sh
scripts/e2e.sh                # ビルド → 起動 → 投稿・一覧・ページ送り・編集・削除を確認 → 停止
E2E_PORT=3104 scripts/e2e.sh  # 待ち受けるポートを変える（既定は 3999）
```

最後に `passed: 67, failed: 0` と出れば成功です。

## just のレシピ

[just](https://github.com/casey/just) を入れると、よく使うコマンドを短く打てます。

| コマンド | 中身 |
|---|---|
| `just` | レシピの一覧を表示 |
| `just run` | `cargo run` |
| `just test` | `cargo test` |
| `just lint` | `cargo clippy --all-targets -- -D warnings` |
| `just fmt-check` | `cargo fmt -- --check` |
| `just verify` | `fmt-check` → `lint` → `test` をまとめて実行（コミット前の確認） |
| `just e2e` | `scripts/e2e.sh`（上の e2e スクリプト） |
| `just e2e create` | e2e のうち、投稿・一覧・ページ送りまで |

## Docker で動かす

```sh
docker compose up --build
```

http://localhost:3000/ を開きます。止めるときは `Ctrl+C`、コンテナを消すときは次のとおりです。

```sh
docker compose down
```

- DB と画像は、リポジトリの `data/` と `uploads/` に保存されます。`down` してから `up` しても投稿は残ります
- コンテナの中のアプリは root ではなく uid 1000 のユーザで動きます。Linux で、リポジトリを置いたユーザの uid が 1000 でない場合は、
  先に `sudo chown 1000:1000 data uploads` で書き込めるようにしてください（`id -u` で自分の uid を確認できます）
- イメージは2段階でビルドします。実行用のイメージ（約 125MB）に入るのはアプリのバイナリ1つだけです。
  テンプレートとマイグレーションはバイナリに埋め込まれています
- e2e スクリプトは、起動しているコンテナにも使えます。`data/` と `uploads/` が空の状態で、次のように実行します

```sh
E2E_BASE_URL=http://127.0.0.1:3000 E2E_UPLOAD_DIR=uploads scripts/e2e.sh
```

## 既知の制限

学習用のため、次の点には手を付けていません。

- 認証と CSRF 対策がありません。誰でもどの投稿でも編集・削除できます
- 日時は UTC のまま表示します
- 同じ投稿を2人が同時に編集すると、画像ファイルが失われることがあります（編集どうしを直列化していないため）
- 編集中に DB の更新だけが失敗すると、画像ファイルと行の内容がずれることがあります（ファイルと DB を1つのトランザクションにできないため）
- 停止時に処理中のリクエストの完了を待ちません（graceful shutdown なし）

## ディレクトリ構成

```text
.
├── src/
│   ├── main.rs          起動処理（.env・ログ・DB 接続・マイグレーション・listen）
│   ├── lib.rs           ルーターの組み立て（build_app）
│   ├── config.rs        ページサイズ・画像の上限などの設定値
│   ├── state.rs         ハンドラで共有する AppState
│   ├── errors.rs        AppError と HTTP ステータス・エラーページへの変換
│   ├── handlers/        HTTP ハンドラ（一覧・作成・編集・削除）とページ計算
│   ├── models/          Post と画像の拡張子 ImageExt
│   ├── repository/      SQL（sqlx）
│   ├── storage/         画像ファイルの保存・削除
│   ├── validators/      投稿フォームの検証
│   └── templates/       Askama のテンプレート構造体
├── templates/           HTML テンプレート（base / index / edit / error）
├── migrations/          テーブル定義（起動時に自動で適用）
├── tests/               統合テスト（common/mod.rs が共通のセットアップ）
├── scripts/e2e.sh       実サーバを curl で確かめるスクリプト
├── data/  uploads/      DB と画像の保存先（中身は git の管理外）
├── Dockerfile  docker-compose.yml  .dockerignore
├── Justfile  .env.example
└── docs/tumigidoc/rust-axum-image-bbs/   要件・設計・タスクの文書
```

## ドキュメント

要件・設計・タスクは [docs/tumigidoc/rust-axum-image-bbs/](docs/tumigidoc/rust-axum-image-bbs/) にあります。

- 要件: [spec/requirements.md](docs/tumigidoc/rust-axum-image-bbs/spec/requirements.md) / 受け入れ基準: [spec/acceptance-criteria.md](docs/tumigidoc/rust-axum-image-bbs/spec/acceptance-criteria.md)
- 設計: [design/architecture.md](docs/tumigidoc/rust-axum-image-bbs/design/architecture.md) / エンドポイント: [design/api-endpoints.md](docs/tumigidoc/rust-axum-image-bbs/design/api-endpoints.md)
- タスク一覧（TASK-0001〜0020）と Zenn 記事の計画: [tasks/overview.md](docs/tumigidoc/rust-axum-image-bbs/tasks/overview.md)

## Zenn 記事

実装の過程を全13回の連載にしています。(1)〜(3) は公開済み、(4)〜(13) は `articles/` に下書き（`published: false`）を置いています。

| 回 | 内容 | タスク |
|---|---|---|
| [(1)](https://zenn.dev/purenium/articles/2026_05_22_rust-axum-imgbbs-01-init) | プロジェクト初期化と Hello, axum! | TASK-0001 |
| [(2)](https://zenn.dev/purenium/articles/2026_05_23_rust-axum-imgbbs-02-tracing) | tracing と TraceLayer で開発体験を整える | TASK-0002 |
| [(3)](https://zenn.dev/purenium/articles/2026_05_25_rust-axum-imgbbs-03-sqlx-migration) | SQLite + sqlx 接続とマイグレーション | TASK-0003 |
| [(4)](articles/2026_11_17_rust-axum-imgbbs-04-appstate.md) | AppConfig / AppState と環境変数読み込み | TASK-0004 |
| [(5)](articles/2026_11_17_rust-axum-imgbbs-05-post-repository.md) | Post モデルとリポジトリ層を sqlx で書く | TASK-0005 |
| [(6)](articles/2026_11_17_rust-axum-imgbbs-06-askama-list.md) | Askama で投稿一覧を表示する | TASK-0006, 0007 |
| [(7)](articles/2026_11_17_rust-axum-imgbbs-07-pagination.md) | ページネーションを実装する | TASK-0008 |
| [(8)](articles/2026_11_17_rust-axum-imgbbs-08-validation-create.md) | バリデーションと multipart での投稿作成 | TASK-0009, 0010 |
| [(9)](articles/2026_11_17_rust-axum-imgbbs-09-image-upload.md) | 画像アップロードとロールバック | TASK-0011 |
| [(10)](articles/2026_11_17_rust-axum-imgbbs-10-app-error.md) | AppError と IntoResponse でエラー処理をまとめる | TASK-0012 |
| [(11)](articles/2026_11_17_rust-axum-imgbbs-11-edit-delete.md) | 編集と削除で CRUD を完成させる | TASK-0013〜0016 |
| [(12)](articles/2026_11_17_rust-axum-imgbbs-12-integration-test.md) | 統合テストで主要シナリオを固める | TASK-0017, 0018 |
| [(13)](articles/2026_11_17_rust-axum-imgbbs-13-docker-readme.md) | Docker と README で配布できる形にする | TASK-0019, 0020 |

## ライセンス

MIT（[LICENSE](LICENSE)）。学習用のプロジェクトです。
