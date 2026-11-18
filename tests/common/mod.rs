//! 統合テスト（tests/*.rs）で共有するセットアップヘルパ。
//!
//! 学習メモ:
//! - `tests/` 直下の `.rs` ファイルは、それぞれが独立したテスト用クレートとしてビルドされる。
//!   `tests/common/mod.rs` はサブディレクトリにあるのでテストクレート扱いされず、
//!   各テストファイルから `mod common;` で取り込んで共有できる。
//! - 取り込んだテストファイルによっては使わない関数があり「未使用」警告が出るため、
//!   このファイルに限り `#[allow(dead_code)]` を付けてよいことにしている。

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, LOCATION};
use axum::http::{Request, StatusCode};
use axum::Router;
use rust_axum_image_bbs::{build_app, config::AppConfig, state::AppState};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use std::cell::RefCell;
use std::sync::{Arc, Mutex, Once};
use tempfile::TempDir;
use tower::ServiceExt; // `oneshot` メソッドを使えるようにするトレイト
use tracing_subscriber::fmt::MakeWriter;

/// テスト1件ぶんのアプリ一式。
pub struct TestApp {
    /// `build_app` で組み立てた本物のルーター。
    pub router: Router,
    /// ルーターと同じ in-memory DB を指すプール。テストデータの投入や検証に使う。
    pub db: SqlitePool,
    /// UPLOAD_DIR として渡した一時ディレクトリ。
    /// `TempDir` は drop されるとディレクトリごと消えるので、テスト中は所有して生かしておく。
    #[allow(dead_code)]
    pub upload_dir: TempDir,
}

/// まっさらな in-memory DB と一時ディレクトリでアプリを組み立てる。
///
/// 学習メモ:
/// - in-memory SQLite は接続が全部閉じると中身が消える。`max_connections(1)` で接続を1本に固定し、
///   マイグレーション・テストデータ投入・ハンドラが必ず同じ DB を見るようにする。
///   （sqlx 0.8 は `sqlite::memory:` を共有キャッシュ付きの in-memory DB に読み替えるので、
///   同じプールなら複数接続でも同じ DB を見る。1本に固定するのは、それに頼らないための安全策）
/// - `db.clone()` はプールの複製だが、中身は参照カウント付きの共有なので同じ接続を指す。
pub async fn setup() -> TestApp {
    // アプリのログを出す前に、ログを集める subscriber を登録しておく（`capture_logs` の説明を参照）
    init_log_capture();
    let db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite に接続できること");
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("マイグレーションが適用できること");

    let upload_dir = TempDir::new().expect("一時ディレクトリを作れること");
    let state = AppState {
        db: db.clone(),
        upload_dir: upload_dir.path().to_path_buf(),
        config: AppConfig::from_env_or_default(),
    };

    TestApp {
        router: build_app(state),
        db,
        upload_dir,
    }
}

/// `GET uri` をルーターへ投げ、ステータスコードと本文（UTF-8 文字列）を返す。
///
/// `oneshot` はルーターの所有権を消費するので、`clone()` した複製に投げる。
/// こうすると同じ `TestApp` に何度でもリクエストできる。
#[allow(dead_code)]
pub async fn get(router: &Router, uri: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

/// `send` / `post_multipart` の結果。
#[allow(dead_code)]
pub struct TestResponse {
    pub status: StatusCode,
    /// `Location` ヘッダ（リダイレクト先）。無ければ `None`。
    pub location: Option<String>,
    /// 本文。HTML を検査しやすいよう文字列にしておく（UTF-8 でない部分は置き換え文字になる）。
    pub body: String,
    /// 本文の生のバイト列（画像の中身を比べるとき用）。
    pub bytes: Vec<u8>,
    /// `Content-Type` ヘッダ。
    pub content_type: Option<String>,
}

/// 任意のリクエストをルーターへ投げ、ステータス・Location・本文を返す。
#[allow(dead_code)]
pub async fn send(router: &Router, request: Request<Body>) -> TestResponse {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let header = |name| {
        response
            .headers()
            .get(name)
            .map(|value: &axum::http::HeaderValue| value.to_str().unwrap().to_string())
    };
    let location = header(LOCATION);
    let content_type = header(CONTENT_TYPE);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    TestResponse {
        status,
        location,
        body: String::from_utf8_lossy(&bytes).into_owned(),
        bytes: bytes.to_vec(),
        content_type,
    }
}

/// UPLOAD_DIR にあるファイル名の一覧（名前順）。
#[allow(dead_code)]
pub fn upload_files(app: &TestApp) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(app.upload_dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// multipart/form-data の1欄。
#[allow(dead_code)]
pub enum Part<'a> {
    /// テキスト欄: (name, 値)
    Text(&'a str, &'a str),
    /// ファイル欄: (name, ファイル名, 中身)
    File(&'a str, &'a str, &'a [u8]),
}

/// ブラウザのフォーム送信と同じ形の multipart/form-data で `POST uri` する。
#[allow(dead_code)]
pub async fn post_multipart(router: &Router, uri: &str, parts: &[Part<'_>]) -> TestResponse {
    const BOUNDARY: &str = "----test-boundary-7MA4YWxkTrZu0gW";
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        match part {
            Part::Text(name, value) => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}")
                    .as_bytes(),
            ),
            Part::File(name, filename, bytes) => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n\
                         Content-Type: application/octet-stream\r\n\r\n"
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(bytes);
            }
        }
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

    let request = Request::post(uri)
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap();
    send(router, request).await
}

/// `post` テーブルの行数。
#[allow(dead_code)]
pub async fn post_count(db: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM post")
        .fetch_one(db)
        .await
        .unwrap()
}

/// テスト中に出たログ（tracing の出力）をためておく入れ物。`capture_logs` で作る。
///
/// 学習メモ:
/// - `Arc<Mutex<Vec<u8>>>` は「複数の持ち主で共有し（Arc）、書き込みは1人ずつ（Mutex）」のバイト列。
///   ログを書く側（subscriber）と読む側（テスト）が同じバッファを指せるよう、clone しても中身は共有される。
#[allow(dead_code)]
#[derive(Clone, Default)]
pub struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

#[allow(dead_code)]
impl CapturedLogs {
    /// ここまでにたまったログ全体（1行 = 1イベント）。
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// tracing-subscriber はログを `std::io::Write` に書き出すので、バッファへ追記する Write を実装する。
impl std::io::Write for CapturedLogs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

thread_local! {
    /// このスレッドのログの行き先。`capture_logs` の間だけ `Some` になる。
    static CAPTURE: RefCell<Option<CapturedLogs>> = const { RefCell::new(None) };
}

/// テストのプロセス全体で1つだけ登録する subscriber の書き込み先。
/// 1行書くたびに `make_writer` が呼ばれるので、そのときのスレッドが集めていればそのバッファへ、
/// 集めていなければ捨てる先（`sink`）へ書く。
struct CurrentThreadLogs;

impl<'a> MakeWriter<'a> for CurrentThreadLogs {
    type Writer = Box<dyn std::io::Write>;

    fn make_writer(&'a self) -> Self::Writer {
        match CAPTURE.with(|capture| capture.borrow().clone()) {
            Some(logs) => Box::new(logs),
            None => Box::new(std::io::sink()),
        }
    }
}

/// INFO 以上のログを人間向けの形式で書く subscriber を、プロセス全体に1回だけ登録する。
///
/// 学習メモ:
/// - tracing は「このログ文を出す価値があるか」をログ文ごとに一度だけ判定して覚えておく（プロセス全体で共有）。
///   テストごとにスレッド専用の subscriber（`set_default`）を付け外しすると、並列に走る別のテストが
///   先にその判定を「誰も聞いていない」で済ませてしまい、ログが1行も集まらないことがあった（約10%で失敗）。
///   subscriber を最初から1つに固定すれば、判定は常にその1つに対して行われる。
/// - `Once` は中の処理を一度だけ実行する。並列に呼ばれても、2回目以降は1回目が終わるのを待ってから素通りする。
fn init_log_capture() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(CurrentThreadLogs)
            .with_ansi(false)
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("subscriber を登録できること");
    });
}

/// `capture_logs` の戻り値。drop されると、このスレッドのログ集めをやめる。
pub struct CaptureGuard;

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        CAPTURE.with(|capture| capture.borrow_mut().take());
    }
}

/// このスレッドで出る INFO 以上のログを集め始める（本番の既定と同じ人間向けの形式）。
///
/// 戻り値の guard を変数に入れておく間だけ集める。`let _ = ...` にすると即 drop されて何も集まらない。
///
/// 学習メモ:
/// - `#[tokio::test]` は既定で1スレッドでテストを動かすので、ハンドラの中で出たログもこのスレッドから書かれる。
///   スレッドごとに行き先を分けているので、並列に走る他のテストのログとは混ざらない。
#[allow(dead_code)]
pub fn capture_logs() -> (CapturedLogs, CaptureGuard) {
    init_log_capture();
    let logs = CapturedLogs::default();
    CAPTURE.with(|capture| *capture.borrow_mut() = Some(logs.clone()));
    (logs, CaptureGuard)
}

/// 投稿を1件 INSERT し、採番された id を返す。
#[allow(dead_code)]
pub async fn insert_post(db: &SqlitePool, title: &str, text: &str, image: Option<&str>) -> i64 {
    sqlx::query("INSERT INTO post (title, text, image) VALUES (?, ?, ?)")
        .bind(title)
        .bind(text)
        .bind(image)
        .execute(db)
        .await
        .expect("INSERT できること")
        .last_insert_rowid()
}
