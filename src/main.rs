//! rust-axum-image-bbs アプリケーションのエントリポイント。
//!
//! このファイルでは axum サーバを 0.0.0.0:3000 で起動し、ルートパス `/` に
//! GET リクエストが来たら "Hello, axum!" を返すだけの最小構成を実装する。
//!
//! 学習メモ:
//! - `#[tokio::main]` マクロは `main()` を tokio の非同期ランタイムでラップする。
//!   これがないと `async fn` を呼び出すための実行環境がないので、軽い見落としに注意。
//! - axum 0.7 系では `axum::Server::bind(...)` ではなく
//!   `tokio::net::TcpListener` と `axum::serve(listener, app)` の組み合わせで起動する。
//!   0.6 系の記事を参考にするときは書き換えが必要。
//! - `?` 演算子は `Result<T, E>` 型の値からエラー時に早期 return するための糖衣構文。
//!   Rust 特有の構文なので、慣れるまでは「失敗時に return Err(e) されるショートカット」と覚える。
//!
//! TASK-0002 追加メモ（tracing / TraceLayer）:
//! - `println!` は手軽だが、本番・テスト・CI でログレベルを制御しにくい。
//!   `tracing::info!` などを使うと RUST_LOG 環境変数一つでレベル切替ができる。
//! - `TraceLayer::new_for_http()` を Router にかぶせると、リクエストの開始・終了・
//!   ステータスコードが自動でログ出力される。ハンドラ側でロギングを書く必要がない。
//!
//! TASK-0003 追加メモ（SQLite + sqlx 接続とマイグレーション）:
//! - `SqlitePoolOptions` は接続プール（複数の DB 接続をまとめて管理する仕組み）を構築する。
//!   プールを使うと、リクエストごとに接続を確立・切断するコストを省いて効率よく DB を使える。
//! - `SqliteConnectOptions::from_str(&url)` は DATABASE_URL 文字列を解析して接続設定を作る。
//!   `?` 演算子により、パース失敗時は即座にエラーを呼び出し元へ返す（early return）。
//! - `create_if_missing(true)` は SQLite ファイルが存在しない場合に自動で作成する設定。
//!   これがないと、ファイルが無いときに `unable to open database file` エラーになる。
//! - `sqlx::migrate!("./migrations")` はコンパイル時マクロで、指定ディレクトリの SQL ファイルを
//!   バイナリに埋め込む。`.run(&pool).await?` で起動時に未適用のマイグレーションだけを実行する。

use axum::{routing::get, Router};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ---------- ロギング初期化 ----------
    //
    // tracing-subscriber の fmt（フォーマット出力）サブスクライバを初期化する。
    // with_env_filter により:
    //   - RUST_LOG 環境変数が設定されていればその値を使う（例: RUST_LOG=debug）
    //   - 未設定の場合は "info,tower_http=debug" をデフォルトとして使う
    //     → アプリ全体は INFO 以上、tower_http（TraceLayer のログ）は DEBUG 以上を表示
    //
    // 学習メモ:
    // - `EnvFilter::try_from_default_env()` は失敗する可能性があるので Result を返す。
    //   `.unwrap_or_else(|_| ...)` で失敗時のフォールバックを指定している。
    //   これは Rust 的な「エラーハンドリングのパターン」として頻出。
    // - `.init()` を呼ぶまでトレースイベントは何も出力されない。
    //   main の最初に書くのは「ここより前のログが消えないようにするため」。
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=debug")),
        )
        .init();

    // ---------- DB 接続プールの構築 ----------
    //
    // DATABASE_URL 環境変数からデータベースのパスを取得する。
    // 環境変数が未設定の場合は "sqlite://data/app.db" をデフォルトとして使う。
    // .env ファイルから読み込む場合は dotenv クレートを使うが、今回はシンプルに std::env で取得する。
    //
    // 学習メモ:
    // - `std::env::var("DATABASE_URL")` は Ok(String) か Err(VarError) を返す。
    //   `.unwrap_or_else(|_| ...)` でエラー時のデフォルト値を文字列リテラルで指定している。
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data/app.db".to_string());

    // SQLite ファイル本体は次の `create_if_missing(true)` で自動生成されるが、
    // **親ディレクトリは作成されない**ため、`data/` が存在しないと
    // `unable to open database file` で起動に失敗する（よくハマるポイント）。
    // ここで `sqlite://` プレフィックスを除いたパスから親を取り出し、念のため作成しておく。
    //
    // 学習メモ:
    // - `if let Some(x) = expr` パターン: Option が Some の時だけブロックを実行する糖衣構文。
    //   `sqlite::memory:` のようなインメモリ URL は strip_prefix が None を返すのでスキップされる。
    // - `std::fs::create_dir_all` は既存ディレクトリに対しても成功する（冪等）。
    if let Some(file_path) = database_url.strip_prefix("sqlite://") {
        if let Some(parent) = std::path::Path::new(file_path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
    }

    // SqliteConnectOptions は接続の詳細設定を表す構造体。
    // `from_str(&database_url)` で URL 文字列をパースして設定を作成する。
    //
    // 学習メモ:
    // - `?` 演算子: Result が Err のとき、即座に main() から return Err(...) する。
    //   ここでは URL のパースに失敗した場合にエラーを伝播させる。
    // - `.create_if_missing(true)`: SQLite ファイル（data/app.db）が存在しない場合に
    //   自動でファイルを作成する。開発環境で DB ファイルを事前に用意しなくてよくなる。
    let connect_options = SqliteConnectOptions::from_str(&database_url)?.create_if_missing(true);

    // SqlitePoolOptions で接続プールを設定・構築する。
    //
    // 学習メモ:
    // - 接続プール（Connection Pool）とは: DB との接続はコストが高い。
    //   プールは事前に複数の接続を確立して使い回す仕組みで、パフォーマンスが向上する。
    // - `max_connections(5)`: 同時に保持できる接続の上限。SQLite はシングルファイルなので
    //   大きくする必要はない。5 本あれば並列リクエストの処理には十分。
    // - `.connect_with(connect_options).await?`: 非同期で接続を試みる。
    //   `await` は非同期処理の完了を待つキーワード（tokio ランタイム上で動作する）。
    //   失敗時は `?` でエラーを伝播する。
    //
    // 型注釈 `SqlitePool` は `sqlx::Pool<sqlx::Sqlite>` のエイリアス。
    // 次タスク TASK-0004 で `AppState { db_pool: SqlitePool, ... }` として持ち回す予定のため、
    // ここで型名を見える化しておく（型推論に任せても動くが、学習者向けに明示）。
    let pool: SqlitePool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connect_options)
        .await?;

    // マイグレーションを実行する。
    //
    // 学習メモ:
    // - `sqlx::migrate!("./migrations")` はコンパイル時マクロ。
    //   パスは `CARGO_MANIFEST_DIR`（= Cargo.toml がある場所）からの相対で解釈されるため、
    //   アプリ実行時のカレントディレクトリに依存しない。
    //   SQL ファイルはビルド時にバイナリへ埋め込まれるので、本番では migrations/ ディレクトリを
    //   配布する必要がない（バイナリ単体で完結する）。
    // - `.run(&pool).await?` は未適用のマイグレーションファイルだけを VERSION 順に実行する。
    //   sqlx は `_sqlx_migrations` テーブルで適用済みを管理するため冪等に動作する。
    sqlx::migrate!("./migrations").run(&pool).await?;
    tracing::info!("migrations applied");

    // 注: pool は現時点ではここで使わない。
    // 次タスク（TASK-0004）で AppState 構造体に格納し、ハンドラへ渡す予定。

    // ---------- ルーター組み立て ----------
    //
    // TraceLayer::new_for_http() は Tower のミドルウェア（Layer）として動作する。
    // Router に `.layer(...)` で重ねると、すべてのリクエスト/レスポンスが
    // このレイヤーを通過するたびに tracing スパンが生成されてログが出る。
    //
    // 学習メモ:
    // - axum の `.layer()` は「ミドルウェアを外側から被せる」イメージ。
    //   複数呼べば複数のレイヤーが積み重なる。
    // - TraceLayer はリクエスト開始時に INFO ログ、終了時に INFO + ステータスを出す。
    //   内部の詳細は DEBUG レベルで出るため、tower_http=debug にしておくと見やすい。
    let app: Router = Router::new()
        .route("/", get(root_handler))
        .layer(TraceLayer::new_for_http());

    // 0.0.0.0:3000 で TCP リスナを開く（WSL からホストアクセスする場合に接続しやすい）
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;

    // 起動時のリスニングアドレスを tracing::info! で出力する。
    // フォーマット・フィルタが他のログと統一されるのが利点（詳細はモジュール冒頭の TASK-0002 メモ参照）。
    tracing::info!("server listening on http://{}", listener.local_addr()?);

    // axum サーバを起動。`?` で I/O エラーを呼び出し元へ伝播する
    axum::serve(listener, app).await?;

    Ok(())
}

/// ルート `/` の GET ハンドラ。固定文字列を返すだけのスタブ。
///
/// 戻り値は `&'static str`。axum は `IntoResponse` を実装した任意の型を返せるので、
/// `&'static str` はそのまま `text/plain; charset=utf-8` として返される。
async fn root_handler() -> &'static str {
    "Hello, axum!"
}
