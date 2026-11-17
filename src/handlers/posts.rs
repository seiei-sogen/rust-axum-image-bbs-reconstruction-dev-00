//! 投稿（post）リソースのハンドラ。
//!
//! ハンドラの責務は「HTTP の入力を受け取る → リポジトリを呼ぶ → ビューモデルに詰めて返す」だけ。
//! SQL はリポジトリ（`repository::post_repo`）、HTML はテンプレート（`templates/`）、
//! ページ計算は `handlers::pagination`、入力の検証は `validators::post_form` に任せる。

use axum::extract::multipart::{Multipart, MultipartRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::errors::{AppError, AppResult};
use crate::handlers::pagination::{paginate, Pagination, PaginationQuery};
use crate::repository::post_repo;
use crate::state::AppState;
use crate::storage::image_store;
use crate::templates::{EditView, IndexView};
use crate::validators::post_form::{validate, PostFormInput, ValidatedImage};

/// 投稿作成に成功したあとのリダイレクト先。一覧の上に「投稿しました」を出す（REQ-202）。
const CREATED_REDIRECT: &str = "/?notice=created";

/// `?notice=created` を受け取るクエリ型。PRG のリダイレクト先で、直前の操作の結果を知らせる。
#[derive(Debug, Deserialize)]
pub struct NoticeQuery {
    pub notice: Notice,
}

/// 一覧の上に出す成功メッセージの種類。
///
/// 学習メモ:
/// - メッセージの文面そのものを `?msg=投稿しました` のように URL で受け取ると、
///   誰でも好きな文面を表示させるリンクを作れてしまう。
///   そこで URL には種類（`created`）だけを載せ、文面はサーバ側の固定文に置き換える。
/// - `#[serde(rename_all = "snake_case")]` で、`Created` を URL 上では `created` と書けるようにしている。
///   一覧にない値（`?notice=hello`）は変換に失敗するので、メッセージは出ない。
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Notice {
    Created,
}

impl Notice {
    fn message(self) -> &'static str {
        match self {
            Notice::Created => "投稿しました",
        }
    }
}

/// `GET /?page=N` 投稿一覧（新しい順に1ページぶん）と投稿フォームを描画する。
///
/// 学習メモ:
/// - `State(state): State<AppState>` は「引数の位置でパターンマッチする」書き方。
///   axum が Router に登録された AppState の複製を `State(...)` に包んで渡してくるので、
///   それを分解して中身だけを変数 `state` に受け取っている。
/// - `Option<Query<PaginationQuery>>` にすると、`?page=abc` のように変換できないとき
///   axum は 400 を返さず `None` を渡してくれる。`None` なら 1 ページ目として扱う。
///   `?notice=...` も同じく、無いか不正なら `None`（メッセージなし）になる。
///   1つのクエリ文字列を2つの型で別々に読んでいて、それぞれ自分に関係ないキーは無視する。
/// - axum は `IntoResponse` を実装した任意の型を返せる。
///   Askama の `#[derive(Template)]` は（askama_axum 経由で）テンプレート構造体に `IntoResponse` を実装するので、
///   `IndexView` を返すだけで `text/html; charset=utf-8` のレスポンスになる。
///   `AppResult<IndexView>` の `Err` 側は `AppError` の `IntoResponse` 実装で 500 になる。
pub async fn list(
    State(state): State<AppState>,
    query: Option<Query<PaginationQuery>>,
    notice: Option<Query<NoticeQuery>>,
) -> AppResult<IndexView> {
    // `map_or(既定値, クロージャ)`: Some なら中身に関数を適用し、None なら既定値を使う
    let requested_page = query.map_or(1, |Query(q)| q.page);
    let view = index_view(&state, requested_page).await?;

    // `..view` は「残りのフィールドは view から持ってくる」という構造体更新記法
    Ok(IndexView {
        success_msg: notice.map(|Query(q)| q.notice.message()),
        ..view
    })
}

/// `POST /posts` 投稿を作成する（multipart/form-data）。
///
/// 流れ:
/// 1. multipart を読んで生の入力 `PostFormInput` にする
/// 2. `validate` で検証する。失敗なら入力値を残したままフォームを 400 で再表示（REQ-201）
/// 3. 合格なら INSERT → 画像があれば保存して `image` 列を UPDATE（保存に失敗したら投稿を取り消す）
/// 4. 303 See Other で一覧へリダイレクト（PRG パターン）
///
/// 学習メモ:
/// - PRG（Post / Redirect / Get）: POST の応答で直接 HTML を返すと、ブラウザの再読み込みで
///   同じ POST がもう一度送られ、二重投稿になる。303 で GET に切り替えさせればそれが起きない。
/// - `Redirect::to` は 303 See Other を返す。303 は「次は GET で取りに来て」という意味が明確。
/// - 引数を `Result<Multipart, MultipartRejection>` で受けると、Content-Type が multipart でないときも
///   axum の既定の応答ではなく、自前の `AppError` を通して返せる（`?` で `From` 変換される）。
/// - `Multipart` はリクエストのボディを読み進める抽出器なので、引数の **最後** に置く決まりがある。
pub async fn create(
    State(state): State<AppState>,
    multipart: Result<Multipart, MultipartRejection>,
) -> AppResult<Response> {
    let mut multipart = multipart?;
    let input = parse_multipart(&mut multipart).await?;

    // `validate` は入力を借用（&input）するだけなので、失敗したときも `input` はまだ手元にある
    let form = match validate(&input, &state.config) {
        Ok(form) => form,
        Err(AppError::Validation(errors)) => {
            // multipart は一度読んだら読み直せないので、さっき作った `input` をそのままフォームに戻す
            let view = IndexView {
                form: input,
                errors,
                ..index_view(&state, 1).await?
            };
            return Ok((StatusCode::BAD_REQUEST, view).into_response());
        }
        Err(err) => return Err(err),
    };

    // 画像のファイル名には投稿 id を使うので、先に画像なしで INSERT して id を決める
    let id = post_repo::insert(&state.db, form.title(), form.text()).await?;

    // `if let Some(x) = ...` は Option が Some のときだけ中に入る書き方
    if let Some(image) = form.image() {
        if let Err(err) = attach_image(&state, id, image).await {
            // 画像を保存できなかった投稿は残さない（EDGE-005）。元のエラー（多くは I/O の 500）を返す
            rollback_create(&state, id).await;
            return Err(err);
        }
    }

    Ok(Redirect::to(CREATED_REDIRECT).into_response())
}

/// 画像を `{id}.{拡張子}` で保存し、そのファイル名を投稿の `image` 列に記録する。
async fn attach_image(state: &AppState, id: i64, image: &ValidatedImage) -> AppResult<()> {
    let filename = image_store::save_image(&state.upload_dir, id, image).await?;
    post_repo::update_image_path(&state.db, id, Some(&filename)).await
}

/// 画像の保存に失敗した投稿を取り消す（補償処理）。書きかけのファイルと INSERT 済みの行を消す。
///
/// SQLite の1文はそれぞれ原子的に実行されるが、「INSERT → ファイル保存 → UPDATE」の3つをまとめて
/// 取り消す仕組みはないので、失敗したら自分で DELETE して元に戻す。
/// ここでの失敗はログにだけ残す。呼び出し元は最初のエラーのほうを利用者に返す。
async fn rollback_create(state: &AppState, id: i64) {
    tracing::warn!(
        post_id = id,
        "image could not be saved, rolling back the post"
    );
    // `if let Err(err) = ...` は「失敗したときだけ」処理する書き方。成功（Ok）なら何もしない
    if let Err(err) = image_store::delete_image(&state.upload_dir, id).await {
        tracing::warn!(post_id = id, error = ?err, "rollback: failed to remove image file");
    }
    if let Err(err) = post_repo::delete(&state.db, id).await {
        tracing::warn!(post_id = id, error = ?err, "rollback: failed to delete post");
    }
}

/// `GET /posts/{id}/edit` 編集フォームを、今の title / text / 画像を入れた状態で表示する（REQ-008）。
///
/// 学習メモ:
/// - `Path(id): Path<i64>` は、ルート `/posts/:id/edit` の `:id` 部分を i64 に変換して受け取る抽出器。
///   `/posts/abc/edit` のように数値にできない値なら、ハンドラは呼ばれず axum が 400 を返す。
/// - `post_repo::find` は `AppResult<Option<Post>>` を返す。
///   1つ目の `?` で DB エラー（Err）を返し、`ok_or(AppError::NotFound)` で「見つからない（None）」を
///   `Err(AppError::NotFound)` に変え、2つ目の `?` でそれを返す。結果、`post` には中身の `Post` だけが入る。
pub async fn edit_form(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<EditView> {
    let post = post_repo::find(&state.db, id)
        .await?
        .ok_or(AppError::NotFound)?;
    // `post` はこの後 EditView へムーブするので、フォーム用の値は clone で複製しておく
    let form = PostFormInput {
        title: post.title.clone(),
        text: post.text.clone(),
        ..PostFormInput::default()
    };
    Ok(EditView {
        post,
        form,
        errors: Vec::new(),
    })
}

/// 一覧ページ（投稿フォームつき）の表示内容を組み立てる。フォームは空、メッセージはなし。
///
/// 学習メモ:
/// - 引数 `state: &AppState` は借用。ハンドラが持っている `state` を貸すだけなので複製は起きない。
/// - `post_repo::list(&state.db, ...)` の `&` も借用。プールは `state` が持ったまま貸すだけ。
async fn index_view(state: &AppState, requested_page: u32) -> AppResult<IndexView> {
    let page_size = state.config.page_size;

    // 範囲外のページを最終ページに丸めるため、先に総件数を数えてから OFFSET を決める
    let total = post_repo::count(&state.db).await?;
    // 構造体のフィールドを一度に取り出す「分割代入（destructuring）」
    let Pagination {
        page,
        total_pages,
        offset,
    } = paginate(requested_page, total, page_size);
    let posts = post_repo::list(&state.db, page_size, offset).await?;

    Ok(IndexView {
        posts,
        page,
        total_pages,
        form: PostFormInput::default(),
        errors: Vec::new(),
        success_msg: None,
    })
}

/// multipart/form-data の各欄を読み、生の入力 `PostFormInput` にまとめる。
///
/// 学習メモ:
/// - `multipart.next_field().await?` は次の欄（パート）を1つずつ返し、最後まで読むと `Ok(None)` を返す。
///   `while let Some(field) = ...` は「Some の間だけ繰り返す」ループの定番の書き方。
/// - `field.text()` / `field.bytes()` は `field` の所有権を消費する（self を値で受け取る）。
///   一方 `field.name()` は借用して &str を返すだけなので、先に名前で振り分けてから中身を読む。
/// - ボディが上限を超えると、ここでの読み取りがエラーになり `?` で `AppError::PayloadTooLarge`（413）に変わる。
async fn parse_multipart(multipart: &mut Multipart) -> AppResult<PostFormInput> {
    let mut input = PostFormInput::default();

    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("title") => input.title = field.text().await?,
            // ブラウザは textarea の改行を CRLF（2文字）で送ってくる。
            // 画面の maxlength は改行を1文字と数えるので、そろえるために LF に直しておく
            Some("text") => input.text = field.text().await?.replace("\r\n", "\n"),
            Some("image") => {
                // `to_owned()` で借用中の &str を所有する String に複製し、この後 `field` を消費できるようにする
                let filename = field.file_name().unwrap_or_default().to_owned();
                let bytes = field.bytes().await?;
                // ファイルを選ばずに送信すると、ブラウザは filename="" で中身が空のパートを送ってくる。
                // それは「画像なし」と扱い、ファイル名つきの 0 バイトファイルはバリデータで拒否させる
                if !(filename.is_empty() && bytes.is_empty()) {
                    input.image_bytes = Some(bytes.to_vec());
                    input.image_filename = Some(filename);
                }
            }
            // チェックボックスは ON のときだけ送られてくるので、欄があれば true
            Some("delete_image") => input.delete_image = true,
            // 知らない欄は読み飛ばす（次の next_field で自動的にスキップされる）
            _ => {}
        }
    }

    Ok(input)
}
