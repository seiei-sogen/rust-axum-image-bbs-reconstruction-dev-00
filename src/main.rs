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

use axum::{routing::get, Router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ルーターを組み立てる：パス `/` への GET を root_handler に紐づける
    let app: Router = Router::new().route("/", get(root_handler));

    // 0.0.0.0:3000 で TCP リスナを開く（WSL からホストアクセスする場合 0.0.0.0 が安全）
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("listening on http://{}", listener.local_addr()?);

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
