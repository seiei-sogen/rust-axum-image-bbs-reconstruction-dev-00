---
title: "Rust + axum で画像掲示板を作る (5) Post モデルとリポジトリ層を sqlx で書く"
emoji: "🗃️"
type: "tech"
topics: ["rust", "axum", "sqlx", "sqlite", "test"]
published: false
---

## はじめに

[前回](./2026_11_17_rust-axum-imgbbs-04-appstate.md) は `AppState` を作り、接続プールをハンドラまで届けた。今回はその接続プールを使って、`post` テーブルを **Rust の型として読み出す層** を作る。

具体的にはこの5つを進める。

- `src/models/post.rs` に `Post` 構造体を定義し、SELECT 結果の1行を受け取れるようにする
- `src/repository/post_repo.rs` に `list` / `count` / `find` の3関数を書く
- `src/errors.rs` に、アプリ共通のエラー型 `AppError` と `AppResult` を最小限で作る
- `src/lib.rs` を新設し、新しいモジュールをライブラリクレートとして公開する
- インメモリ SQLite に対して、リポジトリのテストを6件書く

今回もタスクファイルと実装が食い違う。タスクファイルは `sqlx::query_as!` マクロで SQL をコンパイル時に検証する計画だったが、実装は実行時 API の `sqlx::query_as::<_, Post>(...)` を使っている。理由は後の節で書く。

## 今回のファイル構成

```
src/
├── main.rs            （変更なし）
├── config.rs          （変更なし）
├── state.rs           （変更なし）
├── lib.rs             ← 新規
├── errors.rs          ← 新規
├── models/
│   ├── mod.rs         ← 新規（pub mod post; だけ）
│   └── post.rs        ← 新規
└── repository/
    ├── mod.rs         ← 新規（pub mod post_repo; だけ）
    └── post_repo.rs   ← 新規
```

ディレクトリでモジュールを作るときは、その中の `mod.rs` が本体になり、`pub mod post;` で隣の `post.rs` を子モジュールにする。

## なぜリポジトリ層を切り出すのか

`repository/mod.rs` のコメントには「ハンドラから SQL を追い出してここへ集約することで、『HTTP の都合』と『DB の都合』を別々のファイルで読めるようにする」とある。

ハンドラに SQL 文字列まで混ざると、1つの関数が HTTP と DB の両方を知ることになる。SQL を `post_repo.rs` に集めれば、ハンドラは `post_repo::list(...)` を呼ぶだけで済む。

テストもしやすくなる。リポジトリの関数は `AppState` ではなく `&SqlitePool` を受け取るので、テストでは接続プールを1つ作って渡すだけでよい。

「リポジトリ」と聞くとトレイトや構造体を想像するかもしれないが、ここでは **ただの関数を並べたモジュール** だ。差し替え用のトレイトは作らず、テストでも本物の SQLite に SQL を流す。

## `src/lib.rs` を置いてライブラリクレートにする

```rust:src/lib.rs
//! rust-axum-image-bbs のライブラリクレート。
//!
//! 学習メモ:
//! - Cargo は `src/lib.rs` があると「ライブラリクレート」、`src/main.rs` があると
//!   「バイナリクレート」として扱う。両方置くと、同じパッケージに2つのクレートが同居する。
//! - モジュールをライブラリ側で `pub mod` として公開しておくと、
//!   バイナリ（main.rs）や `tests/` 配下の統合テストから `rust_axum_image_bbs::...` で参照できる。
//! - バイナリ側だけで `mod xxx;` と宣言した場合、まだ main から呼ばれていない関数は
//!   「未使用（dead_code）」警告になる。ライブラリで `pub` にした関数は外部から使われうるので警告にならない。

pub mod errors;
pub mod models;
pub mod repository;
```

**クレート** は Rust のコンパイル単位だ。前回までは `main.rs` を根とするバイナリクレート1つだった。`lib.rs` を置くとライブラリクレートが増え、その名前はハイフンをアンダースコアに変えた `rust_axum_image_bbs` になる。

### なぜ今 `lib.rs` が要るのか

今回の `list` / `count` / `find` は、まだどのハンドラからも呼ばれない。前回までと同じく `main.rs` に `mod repository;` と書くと、未使用の関数として `dead_code` 警告が出る。そして `Justfile` の Lint は警告をエラーとして扱う。

```makefile:Justfile
# Lint。警告はエラー扱いにして見逃さないようにする
lint:
    cargo clippy -- -D warnings
```

試しに `lib.rs` を外し、`main.rs` に `mod errors;` / `mod models;` / `mod repository;` を足して流してみた（一部省略）。

```
error: enum `AppError` is never used
  --> src/errors.rs:17:10
   |
17 | pub enum AppError {
   |          ^^^^^^^^
   |
   = note: `-D dead-code` implied by `-D warnings`
...
error: could not compile `rust-axum-image-bbs` (bin "rust-axum-image-bbs") due to 6 previous errors
```

`AppError` / `AppResult` / `Post` / `list` / `count` / `find` の6つがすべてエラーになった。`#[allow(dead_code)]` で黙らせる手もあるが、それだと本当に不要になったコードも見逃す。コミットメッセージにも「バイナリ側だと未使用警告で clippy が落ちるため」とある。

:::message
この時点の `main.rs` はライブラリを使っておらず、`config` と `state` はバイナリクレート側に残っている。そのため `post_repo.rs` の `use crate::errors::AppResult;` の `crate` は `lib.rs` を指す。前回の `state.rs` の `crate` は `main.rs` だった。同じ `crate::` でも、ファイルがどちらのクレートに属するかで根が変わる。
:::

## `AppError` と `AppResult` を用意する

リポジトリの関数は DB の失敗を呼び出し元へ返す。そのための型を作る。

```rust:src/errors.rs
//! アプリ共通のエラー型 `AppError` と、その `Result` 別名 `AppResult` を定義するモジュール。
//!
//! 現時点では DB 操作の失敗（`sqlx::Error`）だけを扱う。
//! 画像 I/O・バリデーション・404 などのバリアントは、必要になったタスクで順に追加していく。
// ...（学習メモは省略）

/// アプリ全体で使うエラー型。
///
/// `enum`（列挙型）にしておくと、「どの種類の失敗か」を `match` で網羅的に分岐できる。
/// `#[derive(Debug)]` はテストの `unwrap()` 失敗時やログ出力で中身を表示するために必要。
#[derive(Debug)]
pub enum AppError {
    /// DB 操作の失敗。sqlx が返したエラーをそのまま包んで持ち運ぶ。
    Sqlx(sqlx::Error),
}

/// `sqlx::Error` → `AppError` への変換。
// ...（学習メモは省略）
impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Sqlx(err)
    }
}

/// アプリ全体の標準 `Result` 型。
///
/// 毎回 `Result<Vec<Post>, AppError>` と書く代わりに `AppResult<Vec<Post>>` と書ける。
pub type AppResult<T> = Result<T, AppError>;
```

`AppError` は **列挙型（enum）** で、`AppError::Sqlx(sqlx::Error)` のようにバリアントごとに中身を持てる。今はバリアントが1つだけだ。

`impl From<sqlx::Error> for AppError` は、`sqlx::Error` から `AppError` を作る変換の定義だ。これがあると `?` 演算子が変換を引き受ける。`AppResult` を返す関数の中で `sqlx` の呼び出しに `?` を付けると、失敗時は次の順で動く。

1. `sqlx` の呼び出しが `Err(sqlx::Error)` を返す
2. `?` が `From::from` を呼び、`AppError::Sqlx(..)` に包む
3. 包んだ値を `Err(..)` として関数から即座に return する

`AppResult<T>` は **型エイリアス** で、長い型名の別名にすぎない。`#[derive(Debug)]` は飾りではない。テストの `Result::unwrap()` は `Err` の中身を表示して panic するので、エラー型に `Debug` が要る。

:::message
`From` は手書きした。この時点の `Cargo.toml` にエラー型を自動生成するクレートは入っておらず、バリアント1つなら5行で済む。`AppError` は画像アップロードやバリデーションの回でバリアントが増え、第10回で `IntoResponse` を実装して HTTP レスポンスへの変換をまとめる。今はまだ `Display` も `std::error::Error` も実装していない。
:::

## `Post` 構造体と `sqlx::FromRow`

```rust:src/models/post.rs
//! 投稿エンティティ。旧PHP `post` テーブルと一対一で対応する。
// ...（学習メモは省略。本文で説明する）

use chrono::NaiveDateTime;

/// 投稿1件を表す構造体。
///
/// `Clone` を派生しておくと、テンプレートへ渡すときなどに `.clone()` で複製できる。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Post {
    // ...（各フィールドのコメントは省略）
    pub id: i64,
    pub title: String,
    pub text: String,
    pub image: Option<String>,
    pub regist_date: NaiveDateTime,
    pub update_date: NaiveDateTime,
}
```

第3回のマイグレーションと並べると、型の対応はこうなる。

| カラム | SQLite 側の定義 | Rust の型 |
|---|---|---|
| `id` | `INTEGER PRIMARY KEY AUTOINCREMENT` | `i64` |
| `title` | `TEXT NOT NULL` | `String` |
| `text` | `TEXT NOT NULL DEFAULT ''` | `String` |
| `image` | `TEXT`（NULL 可） | `Option<String>` |
| `regist_date` / `update_date` | `TEXT NOT NULL DEFAULT (datetime('now'))` | `NaiveDateTime` |

SQLite の INTEGER は 64bit なので `id` は `i64` だ。`image` だけが `Option<String>` になっている。`Option<T>` は「値がある（`Some(T)`）か、ない（`None`）か」を表す列挙型で、Rust には null が無いので DB の NULL はこれで受ける。NULL になりうるカラムを `String` で受けると、その行を読んだときに実行時エラーになる。

日時は SQLite では `"2026-05-22 14:30:00"` のような文字列だ。第3回で sqlx の `chrono` feature を有効にしたので、`chrono::NaiveDateTime` に変換して受け取れる。`Naive` はタイムゾーンを持たないという意味で、値は UTC で保存されている。

`#[derive(sqlx::FromRow)]` は、SELECT 結果の1行から `Post` を作るコードをコンパイラに書かせる。対応付けは **列名とフィールド名** で行う。両者がずれていても、それが分かるのは行を読んだ時点、つまり実行時だ。この点は後の「実行時 API を選んだ理由」につながる。

## `list` / `count` / `find` を書く

3関数は同じ形をしている。どれも `async fn` で、DB の I/O を `.await` で待つ間、tokio ランタイムは別のリクエストを進められる。接続プールは `&SqlitePool` で借りるだけなので、所有権は呼び出し元の `AppState` に残る。戻り値は `AppResult<...>` だ。

### `list`: 新しい順に `limit` 件

```rust:src/repository/post_repo.rs
/// 投稿を id 降順（新しい順）で取得する。
///
/// - `limit`: 最大何件取るか（1ページの件数）
/// - `offset`: 先頭から何件読み飛ばすか（ページ送り）
///
/// `u32`（符号なし整数）で受けるので、負の値はそもそも渡せない。
pub async fn list(db: &SqlitePool, limit: u32, offset: u32) -> AppResult<Vec<Post>> {
    // `query_as::<_, Post>` の `_` は「DB の種類は推論に任せる」という意味（ここでは SQLite）。
    // `?` プレースホルダに `.bind()` で値を順に流し込む。値は SQL 文字列に埋め込まれず
    // 別送されるので、SQL インジェクションが起きない（NFR-101）。
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

`query_as::<_, Post>` の `::<...>` は **ターボフィッシュ** と呼ばれる書き方で、ジェネリック関数の型引数を明示する。1つ目は DB の種類で、`_` なら推論に任せる。`.fetch_all(db)` に `&SqlitePool` を渡すので SQLite に決まる。2つ目の `Post` は1行を受け取る型で、`FromRow` を実装している必要がある。

SQL の `?` は **プレースホルダ** で、`.bind(limit)` と `.bind(offset)` が先頭から順に値を当てはめる。値は SQL 文字列に連結されず、別に DB へ渡される。値に何が入っていても SQL の構文としては解釈されないので、SQL インジェクションが起きない。

1行目の末尾の `\` は、改行と次の行の先頭の空白を読み飛ばす文字列リテラルの機能だ。`\` の前の空白1つで `update_date FROM post` とつながる。

`.fetch_all(db)` は全行を `Vec<Post>` で返し、`.await?` が失敗を `AppError` にして返す。引数が `u32` なので負の値は渡せず、ページ番号の不正値の扱いは第7回でハンドラ側の仕事になる。

### `count`: 総件数

```rust:src/repository/post_repo.rs
/// 投稿の総件数を返す（ページ数の計算に使う）。
pub async fn count(db: &SqlitePool) -> AppResult<u32> {
    // `query_scalar` は「1行1列」の結果を値そのものとして受け取る API。
    // SQLite の COUNT(*) は 64bit 整数だが、`u32` を指定すると sqlx が範囲チェック付きで変換してくれる
    // （`as` キャストと違い、範囲外なら黙って切り捨てずにエラーになる）。
    let total = sqlx::query_scalar::<_, u32>("SELECT COUNT(*) FROM post")
        .fetch_one(db) // 必ず1行返るクエリなので fetch_one
        .await?;
    Ok(total)
}
```

`COUNT(*)` は1行1列なので、構造体を通さず `query_scalar` で値を直接受け取る。0件でも `0` を1行返すので `.fetch_one(db)` で足りる。

タスクファイルの計画は `query!` で行を受け取り、`row.cnt as u32` で変換する形だった。`as` キャストは範囲外の値を黙って切り捨てる。sqlx の SQLite ドライバのソースを読むと、`u32` の読み出しは 64bit 整数を `try_into()` で変換しており、範囲外ならエラーを返す。実装はこちらに任せた。

### `find`: id で1件

```rust:src/repository/post_repo.rs
/// id で投稿を1件取得する。見つからなければ `Ok(None)`。
///
/// 学習メモ:
/// - 戻り値 `AppResult<Option<Post>>` は二段構え。
///   `Err(..)` = DB が壊れた等の「失敗」、`Ok(None)` = 正常に探したが「該当なし」、`Ok(Some(post))` = 見つかった。
///   「ない」と「エラー」を型で区別できるので、呼び出し側で 404 と 500 を取り違えにくい。
pub async fn find(db: &SqlitePool, id: i64) -> AppResult<Option<Post>> {
    // `fetch_optional` は 0行なら `None`、1行なら `Some(Post)` を返す。
    let post = sqlx::query_as::<_, Post>(
        "SELECT id, title, text, image, regist_date, update_date FROM post WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(post)
}
```

`Result` の中に `Option` が入った二段構えで、取りうる値は3通りある。

| 値 | 意味 | ハンドラが返すもの |
|---|---|---|
| `Ok(Some(post))` | 見つかった | 投稿の内容 |
| `Ok(None)` | 正常に探したが、その id の投稿は無い | 404 |
| `Err(AppError::Sqlx(..))` | DB の操作そのものが失敗した | 500 |

`fetch_one` を使うと、0行のときも `Err` になる。それでは「存在しない」と「DB が壊れた」が同じ `Err` に混ざる。`fetch_optional` で `Option` に分けておけば、呼び出し側は `match` で3通りを書き分けられ、書き忘れはコンパイラが指摘する。タスクファイルでは、編集画面のハンドラが `None` を 404 に変える計画だ。

## 実行時 API を選んだ理由

タスクファイルは `list` をこう書く計画だった。

```rust
sqlx::query_as!(Post, "SELECT id, title, text, image, regist_date, update_date FROM post ORDER BY id DESC LIMIT ? OFFSET ?", limit, offset).fetch_all(db).await?
```

`query_as!` は **マクロ** で、コンパイル時に実際の DB へ SQL を問い合わせ、列の型と `Post` のフィールドを突き合わせる。間違いは `cargo build` の段階で分かるが、そのためにコンパイル時に DB が要る。この書き方の関数を足し、`DATABASE_URL` 無しで `cargo build` するとこうなった。

```
error: set `DATABASE_URL` to use query macros online, or run `cargo sqlx prepare` to update the query cache
   --> src/repository/post_repo.rs:195:17
...
error: could not compile `rust-axum-image-bbs` (lib) due to 1 previous error
```

`post_repo.rs` 冒頭のコメントに、プロジェクトの判断が書いてある。

```rust:src/repository/post_repo.rs
//! 設計判断（sqlx のクエリ API について）:
//! - このプロジェクトでは `sqlx::query_as::<_, Post>("...").bind(x)` のような **実行時 API** を使い、
//!   `query!` / `query_as!` マクロは使わない。
//! - マクロ版は SQL とカラム型をコンパイル時に検証してくれる反面、ビルド時に生きた DB か
//!   `.sqlx/` キャッシュ（`cargo sqlx prepare` で生成）が必要になる。
//!   それだと `git clone && cargo run` や Docker ビルドがそのままでは通らない。
//! - 代償として、SQL の書き間違いやカラム型の不一致は **実行時** にしか分からない。
//!   その穴は、下の `tests` モジュールで全クエリを実際の SQLite に流して埋める。
```

第3回で「`cargo run` だけで DB が組み上がる」ようにしたが、その DB を作るにはまずビルドが通る必要がある。`git clone` 直後や Docker のビルド中には DB が無い。`.sqlx/` をコミットする手もあるが、SQL を変えるたびに `cargo sqlx prepare` を流し直す手順が増える。

失うコンパイル時の検証は、次の節のテストで補う。SQL インジェクション対策はどちらの API でもプレースホルダと `.bind()` で効くので、この選択では変わらない。

## インメモリ SQLite でテストする

テストは `post_repo.rs` の末尾にある。`#[cfg(test)]` を付けたモジュールは `cargo test` のときだけコンパイルされる。

### テストヘルパ

```rust:src/repository/post_repo.rs
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// マイグレーション適用済みの in-memory SQLite プールを作る。
    ///
    /// 学習メモ:
    /// - `sqlite::memory:` はファイルを作らずメモリ上だけに DB を置く。テストごとにまっさらな DB になる。
    /// - SQLite 本体の in-memory DB は「接続」ごとに別々の DB になり、接続が全部閉じると中身も消える。
    ///   sqlx 0.8 は `sqlite::memory:` を、`connect` の呼び出しごとに名前を変えた
    ///   「共有キャッシュ付きの in-memory DB」に読み替えるので、同じプールの接続どうしは同じ DB を見る。
    /// - それでも `max_connections(1)` で接続を1本に固定している。こうしておけば sqlx のこの読み替えに
    ///   頼らなくても、マイグレーションとクエリが必ず同じ DB を見る（必須ではなく安全策）。
    async fn setup_memory_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite に接続できること");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("マイグレーションが適用できること");
        pool
    }

    /// テスト用に投稿を1件 INSERT し、採番された id を返す。
    async fn insert_post(pool: &SqlitePool, title: &str) -> i64 {
        sqlx::query("INSERT INTO post (title, text) VALUES (?, ?)")
            .bind(title)
            .bind("本文")
            .execute(pool)
            .await
            .expect("INSERT できること")
            .last_insert_rowid()
    }

    /// `Vec<Post>` から id だけを取り出す（比較しやすくするため）。
    fn ids(posts: &[Post]) -> Vec<i64> {
        posts.iter().map(|post| post.id).collect()
    }
    // ...（省略。テスト本体は後述）
```

`use super::*;` は親モジュール `post_repo` の中身をすべて取り込む。これで `list` や `Post` をテストからそのまま呼べる。

`setup_memory_pool` は、テストのたびに空の DB を作り、第3回と同じ `sqlx::migrate!` で `post` テーブルを作る。テストごとに別の DB なので、テストどうしが件数を狂わせ合わない。`.expect("...")` は `.unwrap()` と同じく `Err` なら panic し、メッセージに引数の文字列が付く。

`insert_post` は1件入れて、採番された id を返す。`.last_insert_rowid()` は、直前に INSERT した行の id を `execute` の結果から取り出す。`image` と日時はテーブル定義の既定値（NULL と `datetime('now')`）になる。

`ids` の引数 `&[Post]` は **スライス** で、`&Vec<Post>` をそのまま渡せる。`.iter()` で要素を順に借り、`.map(|post| post.id)` で id に変え、`.collect()` で集める。何に集めるかは戻り値の型 `Vec<i64>` から推論される。

### `max_connections(1)` の理由

SQLite 本体の仕様では、`:memory:` を開くと **接続ごとに独立した DB** ができる。接続 A でマイグレーションを流しても、接続 B からは `post` テーブルが見えない。接続プールは複数の接続を使い回すので、マイグレーションとクエリが別の接続に当たると、テーブルが無いエラーになりうる。ただし sqlx 0.8 はこの問題を先回りして避けている。sqlx 0.8 のソースを読むと、URL の `sqlite::memory:` は `file:sqlx-in-memory-{連番}` という **共有キャッシュ付きのインメモリ DB** に読み替えられる。連番は `connect` を呼ぶたびに変わるので、テストごとに別の DB になり、同じ接続プールの接続どうしは同じ DB を共有する。

:::message
実際に `max_connections(1)` を外して3回流しても、テストは6件とも通った。今の sqlx では `max_connections(1)` は必須ではない。それでも接続を1本に固定しておけば、sqlx のこの読み替えに頼らずに「全部同じ DB」を保証できる。コメントにあるとおり、これは安全策だ。
:::

### テスト本体

1件目は `list` の並び順を確かめる。

```rust:src/repository/post_repo.rs
    #[tokio::test]
    async fn test_list_returns_descending_order() {
        let pool = setup_memory_pool().await;
        let first = insert_post(&pool, "1件目").await;
        let second = insert_post(&pool, "2件目").await;
        let third = insert_post(&pool, "3件目").await;

        let posts = list(&pool, 10, 0).await.unwrap();

        // 新しい投稿（後から入れたもの）が先頭に来る
        assert_eq!(ids(&posts), vec![third, second, first]);
        assert_eq!(posts[0].title, "3件目");
    }
```

普通の `#[test]` 関数は `async` にできない。`#[tokio::test]` を付けると、テスト関数ごとに tokio ランタイムを作って `async` の本体を動かしてくれる。`Cargo.toml` の tokio は `"full"` feature なので、追加の設定は要らない。

`insert_post` が返した id と比べるので、id の具体的な値には依存しない。

残りの5件も同じ形で、中身はこうなる。

| テスト | 確かめること |
|---|---|
| `test_list_respects_limit_offset` | 5件入れて `list(2, 2)` で、3番目と2番目に入れた2件が返る |
| `test_list_with_zero_limit_returns_empty` | `limit = 0` なら空の `Vec` が返る |
| `test_count_returns_total` | 0件で 0、7件入れると 7 |
| `test_find_returns_post_for_existing_id` | 存在する id で `Some(post)` が返り、`image` も読める |
| `test_find_returns_none_for_missing` | 存在しない id で `None` が返る |

タスクファイルが求めたのは4件で、境界値の `limit = 0` と `find` で見つかる側を足して6件になった。見つかる側のテストは `post.image.as_deref()` が `Some("1.jpg")` になることも確かめる。`as_deref()` は `Option<String>` を `Option<&str>` に変えるメソッドで、文字列リテラルと比べるために挟む。

## 動かして確かめる

### リポジトリのテストを流す

```sh
$ cargo test repository::post_repo
```

```
     Running unittests src/lib.rs (target/debug/deps/rust_axum_image_bbs-3f277110f1152675)

running 6 tests
test repository::post_repo::tests::test_find_returns_none_for_missing ... ok
test repository::post_repo::tests::test_find_returns_post_for_existing_id ... ok
test repository::post_repo::tests::test_list_respects_limit_offset ... ok
test repository::post_repo::tests::test_list_with_zero_limit_returns_empty ... ok
test repository::post_repo::tests::test_list_returns_descending_order ... ok
test repository::post_repo::tests::test_count_returns_total ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running unittests src/main.rs (target/debug/deps/rust_axum_image_bbs-008e3dd95e21f665)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s
```

引数 `repository::post_repo` はテスト名の絞り込みで、この文字列を含むテストだけが走る。

`Running unittests` が2回出るのは、`src/lib.rs` と `src/main.rs` が別々のテストバイナリになるからだ。`main.rs` 側の `1 filtered out` は、前回の `config` のテストが絞り込みで外れたものだ。6件の表示順は流すたびに変わった。テストは並列に走るが、テストごとに別の DB なので結果は変わらない。

`cargo clippy -- -D warnings` も、`lib.rs` の節で見たエラーを出さずに `Finished` で終わった。

### SQL を壊すとテストが落ちる

コンパイラが SQL を見ない分、テストが本当に SQL を見ているかを確かめた。`find` の `FROM post` を `FROM posts` に書き換える。ビルドは通り、テストでこうなる（後半の同じ内容は省略）。

```
running 6 tests
test repository::post_repo::tests::test_find_returns_none_for_missing ... FAILED
test repository::post_repo::tests::test_find_returns_post_for_existing_id ... FAILED
...
failures:

---- repository::post_repo::tests::test_find_returns_none_for_missing stdout ----

thread 'repository::post_repo::tests::test_find_returns_none_for_missing' (1684) panicked at src/repository/post_repo.rs:188:43:
called `Result::unwrap()` on an `Err` value: Sqlx(Database(SqliteError { code: 1, message: "no such table: posts" }))
...
test result: FAILED. 4 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

`find` を呼ぶ2件だけが落ち、`no such table: posts` と原因まで出ている。`Sqlx(Database(...))` は `AppError` の `#[derive(Debug)]` による表示で、`?` が `sqlx::Error` を `AppError::Sqlx` に包んだことがここで見える。確認後、SQL は元に戻した。

## 詰まったポイントと気づき

- **モジュールを `main.rs` で取り込むと clippy が落ちる**: 呼ばれていない `pub fn` はバイナリクレートでは未使用扱いになり、`-D warnings` で6件ともエラーになった。途中で `lib.rs` が現れるのはこのためだ
- **`query_as!` はビルド時に DB を要求する**: `DATABASE_URL` が無いと、SQL が正しくても `cargo build` が通らない
- **`sqlite::memory:` は SQLite 本体と sqlx で挙動が違う**: SQLite 本体では接続ごとに別の DB になるが、sqlx 0.8 では接続プール内で DB が共有される。`max_connections(1)` を外してもテストは通ったので、この行は必須ではなく安全策として残している
- **`lib.rs` ができると doc テストが走り出す**: 引数なしの `cargo test` の最後に、今回から `Doc-tests rust_axum_image_bbs` が出る。前回「`lib.rs` が無いので doc テストは実行されない」と書いた件だ。対象はライブラリクレートだけで、`config.rs` はまだバイナリ側にある

## 次回予告

第6回は、今回の `list` が返す投稿を画面に出す。

- Askama を導入し、共通レイアウトの `base.html` と一覧ページの `index.html` を作る
- ルーター組み立てを `build_app` に移し、`main.rs` は起動処理だけにする
- `GET /` で `list` の結果をテンプレートに渡し、投稿一覧を表示する

今回のリポジトリ層が、初めてハンドラから呼ばれる回になる。

- 次回: 「Rust + axum で画像掲示板を作る (6) Askama で投稿一覧を表示する」

## 参考リンク

- [sqlx 0.8: `query_as` 関数](https://docs.rs/sqlx/0.8/sqlx/fn.query_as.html)
- [sqlx 0.8: `query_scalar` 関数](https://docs.rs/sqlx/0.8/sqlx/fn.query_scalar.html)
- [sqlx 0.8: `FromRow` トレイト](https://docs.rs/sqlx/0.8/sqlx/trait.FromRow.html)
- [sqlx 0.8: `query_as!` マクロ](https://docs.rs/sqlx/0.8/sqlx/macro.query_as.html) — オフラインモードと `cargo sqlx prepare` の説明
- [sqlx 0.8: `SqliteConnectOptions`](https://docs.rs/sqlx/0.8/sqlx/sqlite/struct.SqliteConnectOptions.html)
- [SQLite — In-Memory Databases](https://www.sqlite.org/inmemorydb.html)
- [tokio: `#[tokio::test]`](https://docs.rs/tokio/1/tokio/attr.test.html)
- [The Rust Programming Language: パッケージとクレート](https://doc.rust-lang.org/book/ch07-01-packages-and-crates.html)
