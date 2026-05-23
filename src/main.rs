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

use axum::{routing::get, Router};
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
