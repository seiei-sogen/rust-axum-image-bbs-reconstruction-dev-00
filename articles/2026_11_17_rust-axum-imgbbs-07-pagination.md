---
title: "Rust + axum で画像掲示板を作る (7) ページネーションを実装する"
emoji: "📄"
type: "tech"
topics: ["rust", "axum", "serde", "askama", "web"]
published: false
---

## はじめに

[前回](./2026_11_17_rust-axum-imgbbs-06-askama-list.md) は Askama で `GET /` の投稿一覧を描き、HTTP テストを書くところまで進めた。ただし一覧は常に先頭の20件だけだった。今回はそこに **ページネーション** を入れる。

- `?page=N` を構造体 `PaginationQuery` で受け取る
- ページ計算を純粋関数 `paginate` に集める
- ハンドラを `Option<Query<PaginationQuery>>` で受ける形にし、`?page=abc` でも 400 にしない
- テンプレートに「前へ / N / M / 次へ」のページ送りを足す
- ユニットテスト7件と HTTP テスト4件を足す

コミットは `TASK-0008: 一覧のページネーションを実装`（`c6097d5`）の1つだ。

## 今回決める振る舞い

1ページは `AppConfig` の `page_size`（既定 20）件で、投稿は新しい順に並ぶ。`?page` の値ごとの表示はこう決める。

| リクエスト | 表示するページ | ステータス |
|---|---|---|
| `/`（`?page` なし） | 1ページ目 | 200 |
| `/?page=2` | 2ページ目 | 200 |
| `/?page=0` | 1ページ目 | 200 |
| `/?page=abc` / `-1` / 空 | 1ページ目 | 200 |
| `/?page=99`（最終ページより後） | 最終ページ | 200 |

**どんな `?page` が来てもエラーにしない**。URL を手で書き換えた人に 400 を返しても得るものがないので、おかしな値は一番近い有効なページに丸める。

ページ送りは、全体が1ページに収まるとき（投稿0件を含む）は出さない。1ページ目では「前へ」を、最終ページでは「次へ」を出さない。

## `serde` を依存に追加する

axum の `Query<T>` は、`T` が `serde` の `Deserialize` を実装していることを求める。

```toml:Cargo.toml
# serde：Rust の構造体と外部データ（クエリ文字列・JSON など）を相互変換する定番クレート。
#   features = ["derive"] で `#[derive(Deserialize)]` が使えるようになり、
#   axum の `Query<T>` が `?page=2` を構造体 T に変換するときに使われる。
serde = { version = "1", features = ["derive"] }
```

`serde` は axum や sqlx の依存としてすでに `Cargo.lock` に入っていた。ただ、依存の依存は自分のコードから `use` できない。`#[derive(Deserialize)]` を書くには `Cargo.toml` に直接書く必要がある。

## `PaginationQuery` で `?page=N` を受け取る

`src/handlers/pagination.rs` を作り、`src/handlers/mod.rs` に `pub mod pagination;` を足す。

```rust:src/handlers/pagination.rs
//! 一覧画面のページネーション（`?page=N`）。
// ...（省略：モジュールの説明と学習メモ）

use serde::Deserialize;

/// `?page=N` を受け取るクエリ型。
///
/// `#[derive(Deserialize)]` により、axum の `Query` がクエリ文字列からこの構造体を組み立てられる。
#[derive(Debug, Clone, Deserialize)]
pub struct PaginationQuery {
    /// 1始まりのページ番号。`?page` が付いていなければ `default_page()` の 1 になる。
    #[serde(default = "default_page")]
    pub page: u32,
}

/// `#[serde(default = "...")]` には「値を返す関数の名前」を文字列で渡す決まり。
fn default_page() -> u32 {
    1
}
```

`#[derive(Deserialize)]` は、「`page` という名前の値を `u32` に変換して `PaginationQuery` を組み立てる」コードを自動生成させる指定だ。axum の `Query` は `?` より後ろの文字列をこれに渡すので、`/?page=2` は `PaginationQuery { page: 2 }` になる。

トップページは `?page` なしの `/` で開かれることが多い。このとき `page` という項目自体が無く、何も指定しなければ serde は変換に失敗する。`#[serde(default = "default_page")]` を付けると、項目が無いときに `default_page()` の戻り値 1 が入る。`= "..."` を省いた `#[serde(default)]` だと `u32` の `Default` である 0 が入ってしまうので、1 を返す関数を用意している。

型を `u32` にしたのは、ページ番号が負になりえず、リポジトリの `list` / `count` も `u32` で揃っているからだ。

## 計算は純粋関数 `paginate` に集める

```rust:src/handlers/pagination.rs
/// `paginate` の計算結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    /// 実際に表示するページ（1〜total_pages に収めたもの）。
    pub page: u32,
    /// 総ページ数。投稿0件でも 1。
    pub total_pages: u32,
    /// SQL の `OFFSET` に渡す、先頭から読み飛ばす件数。
    pub offset: u32,
}

/// 要求されたページ番号を有効範囲に収め、総ページ数と OFFSET を計算する。
///
/// - `requested_page`: ユーザーが `?page=N` で指定した値（0 や範囲外もありうる）
/// - `total`: 投稿の総件数
/// - `page_size`: 1ページの件数（1 以上。AppConfig の既定は 20）
pub fn paginate(requested_page: u32, total: u32, page_size: u32) -> Pagination {
    // 割り算の切り上げ（45件 / 20件 = 3ページ）。整数だけで計算するので浮動小数の誤差がない。
    // 投稿0件だと 0 ページになってしまうので、最低 1 ページにする。
    let total_pages = total.div_ceil(page_size).max(1);
    // 0 → 1、最終ページより後 → 最終ページ、に丸める。total_pages >= 1 なので clamp の範囲は常に正しい
    let page = requested_page.clamp(1, total_pages);
    // page は 1 以上なので `page - 1` が負（u32 のアンダーフローでパニック）になることはない。
    // また page <= total_pages なので、offset が総件数を超えて u32 からあふれることもない。
    let offset = (page - 1) * page_size;

    Pagination {
        page,
        total_pages,
        offset,
    }
}
```

### `div_ceil` で総ページ数を求める

総ページ数は「総件数 ÷ 1ページの件数」の **切り上げ** だ。45件なら 20 + 20 + 5 で3ページになる。整数の割り算 `45 / 20` は切り捨てで 2 なので足りない。`div_ceil` は符号なし整数の切り上げ割り算で、`45u32.div_ceil(20)` は 3 を返す。

後ろの `.max(1)` は投稿0件への備えだ。`0.div_ceil(20)` は 0 なので、何もしないと「0ページ中の1ページ目」になる。

タスク文書 `TASK-0008.md` では、切り上げを手書きの式にしていた。

```rust
let total_pages = ((total + page_size - 1) / page_size).max(1);
```

結果は同じだが、この式は clippy に指摘される。式だけを置いた関数を作り、手元で `cargo clippy` にかけた出力の警告部分がこれだ。

```
warning: manually reimplementing `div_ceil`
 --> src/lib.rs:2:5
  |
2 |     ((total + page_size - 1) / page_size).max(1)
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider using `.div_ceil()`: `total.div_ceil(page_size)`
  |
  = help: for further information visit https://rust-lang.github.io/rust-clippy/rust-1.97.0/index.html#manual_div_ceil
  = note: `#[warn(clippy::manual_div_ceil)]` on by default
```

`clippy::manual_div_ceil` は既定で有効な lint だ。プロジェクトは前回の TASK-0007 の時点ですでに `div_ceil` で書いており、今回はその行を `paginate` に移した。

手書きの式は、`total + page_size - 1` の足し算があふれうる。`total` に `u32::MAX` を入れて比べると、debug ビルドの手書き版は `attempt to add with overflow` で panic し、`div_ceil` は `214748365` を返した。なお `div_ceil` も割る数が 0 なら panic する。ドキュメントコメントで `page_size` を「1 以上」としているのはこのためだ。

### `clamp` で要求ページを範囲に収める

`clamp(min, max)` は、値を `min` 以上 `max` 以下に収める。`?page=0` は `0.clamp(1, 3)` で 1 に、`?page=99` は `99.clamp(1, 3)` で 3 になる。「0 は1ページ目へ」「最終ページより後は最終ページへ」の2つの規則が1行で済む。

`clamp` は `min > max` のとき panic する。手元で `5u32.clamp(1, 0)` を実行すると `min > max. min = 1, max = 0` で落ちた。投稿0件で `total_pages` が 0 のままだとこの形になるので、前の行の `.max(1)` はこの panic も防いでいる。

### OFFSET を計算する

OFFSET は先頭から読み飛ばす件数で、`(page - 1) * page_size` になる。`u32` の `0 - 1` は表せないので、debug ビルドでは panic する。タスク文書が `saturating_sub(1)`（0 で止まる引き算）を使っていたのはこのためだ。`paginate` では先に `clamp` で `page >= 1` を保証しているので、普通の `-` で足りる。危ない値は入口で丸め、後ろの計算は素直に書く順序にしている。

### タスク文書の `normalized()` / `offset()` を採らなかった理由

タスク文書では、計算を `PaginationQuery` のメソッドにする案だった。

```rust
impl PaginationQuery {
    pub fn normalized(&self) -> u32 { self.page.max(1) }
    pub fn offset(&self, page_size: u32) -> u32 {
        self.normalized().saturating_sub(1) * page_size
    }
}
```

プロジェクトはクエリの型と計算を分け、関数 `paginate` 1つにした。コードとコミットから読み取れる理由は2つある。

1つ目は、**最終ページへの丸めには総件数が要る** ことだ。最終ページが何ページかは DB の総件数で決まり、クエリ文字列しか持たない `PaginationQuery` には分からない。この案だと `?page=99` の OFFSET は 1960 になり、空のページが表示される。

2つ目は、**計算を1か所に集めてテストしやすくする** ことだ。`paginate` は3つの `u32` を受け取って `Pagination` を返すだけで、DB も HTTP も触らない。こうした、同じ入力なら必ず同じ出力を返し外の状態を読み書きしない関数を **純粋関数** と呼ぶ。「SQL はリポジトリ、HTML はテンプレート」に「ページ計算は `pagination`」が加わった形だ。

## `paginate` のユニットテスト

純粋関数なので、テストは `#[test]` の普通の関数で書ける。基準は「1ページ20件・総件数45件」だ。

```rust:src/handlers/pagination.rs
#[cfg(test)]
mod tests {
    use super::*;

    /// 1ページ20件・総件数45件（= 20 + 20 + 5 の3ページ）を基準にする
    const PAGE_SIZE: u32 = 20;

    #[test]
    fn first_page_starts_at_offset_zero() {
        assert_eq!(
            paginate(1, 45, PAGE_SIZE),
            Pagination {
                page: 1,
                total_pages: 3,
                offset: 0
            }
        );
    }

    // ...（省略：2ページ目・3ページ目の OFFSET、page=0 のテスト）

    #[test]
    fn total_pages_rounds_up_partial_pages() {
        assert_eq!(paginate(1, 40, PAGE_SIZE).total_pages, 2);
        assert_eq!(paginate(1, 41, PAGE_SIZE).total_pages, 3);
        assert_eq!(paginate(1, 1, PAGE_SIZE).total_pages, 1);
    }

    #[test]
    fn page_beyond_last_clamps_to_last_page() {
        assert_eq!(paginate(4, 45, PAGE_SIZE), paginate(3, 45, PAGE_SIZE));
        assert_eq!(
            paginate(u32::MAX, 45, PAGE_SIZE),
            paginate(3, 45, PAGE_SIZE)
        );
    }

    #[test]
    fn no_posts_still_has_one_page() {
        let expected = Pagination {
            page: 1,
            total_pages: 1,
            offset: 0,
        };
        assert_eq!(paginate(1, 0, PAGE_SIZE), expected);
        assert_eq!(paginate(5, 0, PAGE_SIZE), expected);
    }
}
```

40件と41件を並べて切り上げの境目を見ている。範囲外のテストは期待値を「`paginate(3, ...)` と同じ」と書き、3つのフィールドすべてが正規の最終ページと一致することを1行で確かめている。

```sh
$ cargo test pagination
```

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.21s
     Running unittests src/lib.rs (target/debug/deps/rust_axum_image_bbs-cfb4e08c40b9b176)

running 7 tests
test handlers::pagination::tests::first_page_starts_at_offset_zero ... ok
test handlers::pagination::tests::page_zero_falls_back_to_first_page ... ok
test handlers::pagination::tests::no_posts_still_has_one_page ... ok
test handlers::pagination::tests::page_beyond_last_clamps_to_last_page ... ok
test handlers::pagination::tests::last_page_skips_all_full_pages ... ok
test handlers::pagination::tests::second_page_skips_one_page_of_posts ... ok
test handlers::pagination::tests::total_pages_rounds_up_partial_pages ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 0.01s

     Running unittests src/main.rs (target/debug/deps/rust_axum_image_bbs-6069c2d44405c9a1)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/integration_list.rs (target/debug/deps/integration_list-0d2d526cae2bdb17)

running 1 test
test pagination_shows_20_posts_then_the_remaining_5 ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out; finished in 0.01s
```

`cargo test <文字列>` は、テスト名にその文字列を含むものだけを走らせる。統合テストの1件も名前が一致したので一緒に走っている。

## ハンドラ `list` を書き換える

```rust:src/handlers/posts.rs
// ...（省略：use 宣言）

/// `GET /?page=N` 投稿一覧（新しい順に1ページぶん）を描画する。
///
/// 学習メモ:
/// ...（省略：State の分解、IntoResponse、借用の学習メモ）
/// - `Option<Query<PaginationQuery>>` にすると、`?page=abc` のように変換できないとき
///   axum は 400 を返さず `None` を渡してくれる。`None` なら 1 ページ目として扱う。
pub async fn list(
    State(state): State<AppState>,
    query: Option<Query<PaginationQuery>>,
) -> AppResult<IndexView> {
    // `map_or(既定値, クロージャ)`: Some なら中身に関数を適用し、None なら既定値を使う
    let requested_page = query.map_or(1, |Query(q)| q.page);
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
    })
}
```

前回から、引数に `query` が増え、`count` と `list` の順番が入れ替わり、`IndexView` の `page` が固定の `1` から計算結果に変わった。

### `Query<T>` だけだと `?page=abc` が 400 になる

`Query<T>` は axum の **extractor**（リクエストから値を取り出す型）で、引数に書くだけでクエリ文字列を `T` に変換して渡してくれる。ただし axum 0.7 の `Query<T>` は、変換に失敗するとハンドラを呼ばずに 400 を返す。試しに引数をタスク文書どおりの `Query(q): Query<PaginationQuery>` に戻して叩くと、こうなった（`date` と `content-length` の行は省いた）。

```sh
$ curl -i "http://localhost:3000/?page=abc"
```

```
HTTP/1.1 400 Bad Request
content-type: text/plain; charset=utf-8

Failed to deserialize query string: invalid digit found in string
```

`?page=-1` も同じメッセージで、空の `?page=` は `cannot parse integer from empty string` で 400 だった。一方 `?page` 自体が無い `/` は `#[serde(default)]` が効くので 400 にならない。

### `Option<Query<T>>` で失敗を `None` として受け取る

axum 0.7 では、extractor を `Option` で包むと「取り出しに失敗したら `None`」という意味になる。axum-core 0.4.5 の `impl<S, T> FromRequestParts<S> for Option<T>` を読むと、本体はこの1行だ。

```rust:axum-core-0.4.5/src/extract/mod.rs
Ok(T::from_request_parts(parts, state).await.ok())
```

中の extractor を実行し、その `Result` を `.ok()` で `Option` に変えている。`Ok(値)` は `Some(値)` に、`Err(拒否理由)` は `None` になり、拒否理由は捨てられる。だから axum は 400 を返さずにハンドラを呼ぶ。

:::message alert
これは axum 0.7（axum-core 0.4）の実装だ。axum を上げるときは、後で見る `invalid_page_falls_back_to_first_page` テストで挙動を確かめ直す。
:::

### `map_or` とクロージャのパターン

```rust
let requested_page = query.map_or(1, |Query(q)| q.page);
```

`Option::map_or(既定値, 関数)` は、`Some(x)` なら `関数(x)` の結果を、`None` なら既定値を返す。`match` で `Some` と `None` の2つの腕を書くのと同じことを1行で書ける。

`|Query(q)| q.page` は **クロージャ**（その場で書く無名関数）で、引数の `Query(q)` は `State(state)` と同じパターンマッチだ。`/` なら `Some` の中の default の 1 が、`?page=abc` なら `None` で既定値の 1 が使われる。

### 総件数を先に数え、分割代入で受け取る

前回は `list` で1ページぶん読んでから `count` で数えていた。今回は逆だ。OFFSET を決める `paginate` が、最終ページへの丸めに総件数を必要とするからだ。「総件数 → OFFSET → 一覧」という依存の順に、コードを並べるしかない。

`let Pagination { page, total_pages, offset } = paginate(...);` は、`let` の左辺に構造体の形を書いてフィールドを一度に変数へ取り出す **分割代入** だ。取り出した `page` と `total_pages` は、同名フィールドの省略記法でそのまま `IndexView { posts, page, total_pages }` に渡している。`IndexView` には前回の時点でこの2つのフィールドがあったので、構造体の定義は変えていない。

## `LIMIT ? OFFSET ?` はリポジトリ層にすでにある

一覧の SQL は、第5回で書いた `post_repo::list` のままで、今回は手を入れていない。

```rust:src/repository/post_repo.rs
pub async fn list(db: &SqlitePool, limit: u32, offset: u32) -> AppResult<Vec<Post>> {
    // ...（省略）
    let posts = sqlx::query_as::<_, Post>(
        "SELECT id, title, text, image, regist_date, update_date \
         FROM post ORDER BY id DESC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(db) // 全行を Vec<Post> として受け取る
    .await?;
    Ok(posts)
}
```

2ページ目なら `LIMIT 20 OFFSET 20` で、新しい順の21件目から40件目が返る。値は `.bind()` で SQL とは別に送るので、URL 由来の値でも SQL インジェクションにはならない。引数が `u32` なので、`paginate` の戻り値をそのまま渡せる。

## Askama でページ送りを描く

`templates/index.html` の一覧の下、`{% endblock %}` の直前に足す。

```html:templates/index.html
{% if total_pages > 1 %}
<nav aria-label="ページ送り">
  <ul class="pagination">
    {% if page > 1 %}
    <li class="page-item"><a class="page-link" href="?page={{ page - 1 }}">前へ</a></li>
    {% endif %}
    <li class="page-item active" aria-current="page"><span class="page-link">{{ page }} / {{ total_pages }}</span></li>
    {% if page < total_pages %}
    <li class="page-item"><a class="page-link" href="?page={{ page + 1 }}">次へ</a></li>
    {% endif %}
  </ul>
</nav>
{% endif %}
```

3つの `{% if %}` が、冒頭で決めた表示の規則にそのまま対応する。`total_pages > 1` で1ページに収まるときはページ送りごと消える。投稿0件でも `total_pages` は 1 なのでここで消える。`page > 1` と `page < total_pages` が「前へ」「次へ」の出し分けだ。

Askama は `{{ page - 1 }}` のように `{{ }}` の中で算術を書ける。テンプレートはコンパイル時に Rust のコードになるので、これは `u32` の引き算として実行される。`page` が 0 ならアンダーフローするが、`paginate` が `page >= 1` を保証し、さらに `{% if page > 1 %}` の内側でしか引き算しないので、0 は来ない。

クラス名は `base.html` で読み込んでいる Bootstrap 5 のものだ。タスク文書のスケッチとは、`aria-label` を「ページ送り」にし、現在のページに `aria-current="page"`（支援技術に現在地を伝える属性）を足した点が違う。

## HTTP テストでつなぎ目を確かめる

ハンドラ・SQL・テンプレートのつなぎ目は、HTTP リクエストを投げないと確かめられない。前回作った `tests/common/mod.rs` を使い、`tests/integration_list.rs` に4件足した。

```rust:tests/integration_list.rs
/// `post-01`（最古）〜 `post-{n}`（最新）のタイトルで n 件投入する。
async fn insert_numbered_posts(db: &sqlx::SqlitePool, n: u32) {
    for i in 1..=n {
        common::insert_post(db, &format!("post-{i:02}"), "本文", None).await;
    }
}

/// カードのタイトル要素。`post-1` が `post-10` に部分一致しないよう、タグごと比較する。
fn title(n: u32) -> String {
    format!(">post-{n:02}</h3>")
}

#[tokio::test]
async fn pagination_shows_20_posts_then_the_remaining_5() {
    let app = common::setup().await;
    insert_numbered_posts(&app.db, 25).await;

    // 1ページ目: 新しい順に post-25 〜 post-06 の20件
    let (status, page1) = common::get(&app.router, "/?page=1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count(&page1, "<article"), 20);
    assert!(page1.contains(&title(25)) && page1.contains(&title(6)));
    assert!(!page1.contains(&title(5)));
    assert!(page1.contains("1 / 2"));
    assert!(page1.contains(r#"href="?page=2""#) && page1.contains("次へ"));
    assert!(!page1.contains("前へ"));

    // ...（省略：2ページ目に post-05 〜 post-01 の5件と「2 / 2」「前へ」が出ること）
}

#[tokio::test]
async fn invalid_page_falls_back_to_first_page() {
    let app = common::setup().await;
    insert_numbered_posts(&app.db, 25).await;
    let (_, first_page) = common::get(&app.router, "/").await;
    assert!(first_page.contains(&title(25)) && first_page.contains("1 / 2"));

    for uri in ["/?page=abc", "/?page=-1", "/?page=0", "/?page="] {
        let (status, html) = common::get(&app.router, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri} は 400 ではなく 200");
        assert_eq!(html, first_page, "{uri} は1ページ目と同じ内容");
    }
}
```

`{i:02}` は「2桁に満たなければ 0 で埋める」書式指定だ。`contains("post-1")` は `post-10` にも一致するので、`>post-01</h3>` とタグごと探している。`r#"..."#` は `"` をエスケープせずに書ける raw 文字列リテラルだ。

1つ目のテストは件数だけでなく、境目の両側を見ている。1ページ目に `post-06` があり `post-05` が無いことを確かめれば、OFFSET が1件ずれたときに気づける。

2つ目のテストは、4種類の不正な値で `/` と **HTML 全体が一致する** ことを確かめる。`abc` / `-1` / 空は `None` になる経路、`0` は `clamp` の経路で、両方を押さえている。

省略した残り2件は、`?page=99` が最終ページになる `page_beyond_last_shows_last_page` と、投稿0件と20件ちょうどでページ送りが出ない `pager_is_hidden_when_everything_fits_on_one_page` だ。

```sh
$ cargo test --test integration_list
```

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.16s
     Running tests/integration_list.rs (target/debug/deps/integration_list-0d2d526cae2bdb17)

running 9 tests
test list_returns_500_when_db_is_unavailable ... ok
test list_shows_empty_message_when_no_posts ... ok
test list_escapes_html_in_posts ... ok
test invalid_page_falls_back_to_first_page ... ok
test list_shows_image_only_for_posts_with_image ... ok
test list_shows_posts_newest_first ... ok
test pager_is_hidden_when_everything_fits_on_one_page ... ok
test page_beyond_last_shows_last_page ... ok
test pagination_shows_20_posts_then_the_remaining_5 ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
```

前回の5件と合わせて9件が通っている。

## 動かして確かめる

投稿が45件あれば 20 + 20 + 5 の3ページになる。前回までの投稿が残っていると件数がずれるので、DB を消してから起動する。

```sh
$ rm -f data/app.db
$ cargo run
```

```
2026-11-17T14:25:42.260775Z  INFO rust_axum_image_bbs: migrations applied
2026-11-17T14:25:42.261060Z  INFO rust_axum_image_bbs: server listening on http://0.0.0.0:3000
```

投稿フォームはまだ動かないので、別のターミナルから45件を直接入れる。`sqlite3` コマンドが無い環境もあるので、Python 標準の `sqlite3` モジュールを使う。

```sh
$ python3 - data/app.db <<'EOF'
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
con.executemany(
    "INSERT INTO post (title, text) VALUES (?, ?)",
    [(f"post-{i:02}", f"{i}件目の本文") for i in range(1, 46)],
)
con.commit()
print(con.execute("SELECT COUNT(*) FROM post").fetchone()[0])
EOF
45
```

`post-01` が最古、`post-45` が最新になる。5つの URL を叩き、カードの数・先頭の投稿・ページ送りの文字を1行ずつ抜き出す。

```sh
$ for q in "" "?page=2" "?page=3" "?page=abc" "?page=99"; do
  html=$(curl -s "http://localhost:3000/$q")
  printf '%-11s cards=%-2s first=%s pager=%s\n' "/$q" "$(grep -c '<article' <<<"$html")" "$(grep -o 'post-[0-9]*' <<<"$html" | head -1)" "$(grep -o -e '前へ' -e '[0-9]* / [0-9]*' -e '次へ' <<<"$html" | paste -sd' ')"
done
```

```
/           cards=20 first=post-45 pager=1 / 3 次へ
/?page=2    cards=20 first=post-25 pager=前へ 2 / 3 次へ
/?page=3    cards=5  first=post-05 pager=前へ 3 / 3
/?page=abc  cards=20 first=post-45 pager=1 / 3 次へ
/?page=99   cards=5  first=post-05 pager=前へ 3 / 3
```

冒頭の表どおりだ。`?page=abc` は1ページ目と、`?page=99` は3ページ目と同じ内容になっている。ステータスコードも見ておく。

```sh
$ for q in "?page=abc" "?page=-1" "?page=0" "?page="; do curl -s -o /dev/null -w "/$q %{http_code}\n" "http://localhost:3000/$q"; done
/?page=abc 200
/?page=-1 200
/?page=0 200
/?page= 200
```

`Query<T>` のままでは 400 だった3つも含めて、すべて 200 になった。

## 詰まったポイントと気づき

- **`Query<T>` は変換に失敗すると 400 を返す**: `?page=-1` や空の `?page=` も 400 になった。`?page` 自体が無いときは `#[serde(default)]` の経路なので、「項目が無い」と「変換できない」は別物として覚えておく
- **`#[serde(default)]` だけだと 0 ページ目になる**: `u32` の `Default` は 0 だ。1始まりにするには `default = "default_page"` で関数を名前指定する
- **手書きの切り上げ式は clippy に指摘される**: タスク文書の `(total + page_size - 1) / page_size` は `clippy::manual_div_ceil` の対象だ。`div_ceil` なら意図が名前で読め、足し算のあふれも無い
- **`u32` の引き算と `clamp` は順番が大事**: `page - 1` は 0 で、`clamp(1, 0)` は `min > max` で panic する。`.max(1)` → `clamp` → 引き算の順に並べればどちらも起きない
- **最終ページへの丸めには総件数が要る**: タスク文書の `offset()` 案では `?page=99` を丸められない。総件数を受け取る `paginate` にし、`count` と `list` の順番も入れ替えた
- **HTML の `contains` は部分一致する**: `post-1` を探すと `post-10` にも一致する。テストでは番号をゼロ埋めし、タグごと探している

## 次回予告

第8回は、いよいよ投稿を作れるようにする。

- タイトル1〜50文字・本文500文字以内・画像の種類（先頭のマジックバイトで判定）を検査するバリデータを作る
- `POST /posts` で `multipart/form-data` を読み取る
- バリデーションで弾いたときは、入力内容とエラーメッセージを残したままフォームを描き直す

今回 Python で直接入れた投稿を、ブラウザのフォームから作れるようになる。

- 次回: 「Rust + axum で画像掲示板を作る (8) バリデーションと multipart での投稿作成」

## 参考リンク

- [axum 0.7 `Query` (docs.rs)](https://docs.rs/axum/0.7/axum/extract/struct.Query.html)
- [axum 0.7 Optional extractors (docs.rs)](https://docs.rs/axum/0.7/axum/extract/index.html#optional-extractors) — extractor を `Option` で包んだときの挙動
- [serde: Field attributes](https://serde.rs/field-attrs.html) — `#[serde(default = "path")]`
- [`u32::div_ceil`](https://doc.rust-lang.org/std/primitive.u32.html#method.div_ceil) / [`Ord::clamp`](https://doc.rust-lang.org/std/cmp/trait.Ord.html#method.clamp)
- [Clippy lint: `manual_div_ceil`](https://rust-lang.github.io/rust-clippy/master/index.html#manual_div_ceil)
- [askama 0.12 (docs.rs)](https://docs.rs/askama/0.12/askama/)
- [Bootstrap 5.3 Pagination](https://getbootstrap.com/docs/5.3/components/pagination/)
