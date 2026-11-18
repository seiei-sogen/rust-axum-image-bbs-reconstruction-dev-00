//! `post` テーブルへのクエリを集約するリポジトリ。
//!
//! 設計判断（sqlx のクエリ API について）:
//! - このプロジェクトでは `sqlx::query_as::<_, Post>("...").bind(x)` のような **実行時 API** を使い、
//!   `query!` / `query_as!` マクロは使わない。
//! - マクロ版は SQL とカラム型をコンパイル時に検証してくれる反面、ビルド時に生きた DB か
//!   `.sqlx/` キャッシュ（`cargo sqlx prepare` で生成）が必要になる。
//!   それだと `git clone && cargo run` や Docker ビルドがそのままでは通らない。
//! - 代償として、SQL の書き間違いやカラム型の不一致は **実行時** にしか分からない。
//!   その穴は、下の `tests` モジュールで全クエリを実際の SQLite に流して埋める。
//!
//! 学習メモ:
//! - 各関数は `async fn`。DB への問い合わせは I/O 待ちが発生するので、`.await` で待っている間に
//!   tokio ランタイムが別のリクエストを処理できる。
//! - 引数 `db: &SqlitePool` は「借用（参照）」。プールの所有権は呼び出し元（AppState）が持ったまま、
//!   この関数は一時的に借りて使うだけなので、呼び出し後も呼び出し元はプールを使い続けられる。
//! - `?` を付けると、`sqlx::Error` は `From` 実装により自動で `AppError::Sqlx` に変換されて return される。

use sqlx::SqlitePool;

use crate::errors::AppResult;
use crate::models::post::Post;

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

/// 画像なしで投稿を1件 INSERT し、採番された id を返す。
///
/// 画像は id が決まってから `{id}.{拡張子}` の名前で保存するので、INSERT の時点では NULL のまま。
/// `regist_date` / `update_date` はテーブル定義の `DEFAULT (datetime('now'))` に任せる。
pub async fn insert(db: &SqlitePool, title: &str, text: &str) -> AppResult<i64> {
    // `execute` は行を返さないクエリ用。戻り値の `SqliteQueryResult` から結果の情報を取り出せる
    let result = sqlx::query("INSERT INTO post (title, text) VALUES (?, ?)")
        .bind(title)
        .bind(text)
        .execute(db)
        .await?;
    // `last_insert_rowid()` は、この INSERT で採番された id（AUTOINCREMENT の値）
    Ok(result.last_insert_rowid())
}

/// 投稿のタイトルと本文を更新する。`image` 列には触らない。`update_date` は今の時刻にする。
///
/// 学習メモ:
/// - `update_date = datetime('now')` のように、時刻は Rust 側で作らず SQLite に作らせる。
///   INSERT 時の `DEFAULT (datetime('now'))` と同じ形式・同じ時計になるので、一覧の表示がそろう。
pub async fn update(db: &SqlitePool, id: i64, title: &str, text: &str) -> AppResult<()> {
    sqlx::query("UPDATE post SET title = ?, text = ?, update_date = datetime('now') WHERE id = ?")
        .bind(title)
        .bind(text)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// 投稿の画像ファイル名（例: `"5.png"`）を設定する。`None` なら NULL（画像なし）にする。
///
/// 学習メモ:
/// - `Option<&str>` を `.bind()` すると、`Some("5.png")` は文字列、`None` は SQL の NULL として送られる。
pub async fn update_image_path(db: &SqlitePool, id: i64, image: Option<&str>) -> AppResult<()> {
    sqlx::query("UPDATE post SET image = ?, update_date = datetime('now') WHERE id = ?")
        .bind(image)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// 投稿を1件削除する。該当行が無くてもエラーにはしない（存在確認は呼び出し側で `find` する）。
pub async fn delete(db: &SqlitePool, id: i64) -> AppResult<()> {
    sqlx::query("DELETE FROM post WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

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

    #[tokio::test]
    async fn test_list_respects_limit_offset() {
        let pool = setup_memory_pool().await;
        let mut inserted = Vec::new();
        for n in 1..=5 {
            inserted.push(insert_post(&pool, &format!("投稿{n}")).await);
        }

        // 新しい順は 5,4,3,2,1。先頭2件を飛ばして2件取ると「3番目・2番目」
        let posts = list(&pool, 2, 2).await.unwrap();

        assert_eq!(ids(&posts), vec![inserted[2], inserted[1]]);
    }

    #[tokio::test]
    async fn test_list_with_zero_limit_returns_empty() {
        let pool = setup_memory_pool().await;
        insert_post(&pool, "1件目").await;

        let posts = list(&pool, 0, 0).await.unwrap();

        assert!(posts.is_empty());
    }

    #[tokio::test]
    async fn test_count_returns_total() {
        let pool = setup_memory_pool().await;
        assert_eq!(count(&pool).await.unwrap(), 0);

        for n in 1..=7 {
            insert_post(&pool, &format!("投稿{n}")).await;
        }

        assert_eq!(count(&pool).await.unwrap(), 7);
    }

    #[tokio::test]
    async fn test_find_returns_post_for_existing_id() {
        let pool = setup_memory_pool().await;
        let id = sqlx::query("INSERT INTO post (title, text, image) VALUES (?, ?, ?)")
            .bind("hello")
            .bind("world")
            .bind("1.jpg")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();

        let post = find(&pool, id).await.unwrap().expect("投稿が見つかること");

        assert_eq!(post.id, id);
        assert_eq!(post.title, "hello");
        assert_eq!(post.text, "world");
        assert_eq!(post.image.as_deref(), Some("1.jpg"));
    }

    #[tokio::test]
    async fn test_find_returns_none_for_missing() {
        let pool = setup_memory_pool().await;
        insert_post(&pool, "1件目").await;

        let post = find(&pool, 999).await.unwrap();

        assert!(post.is_none());
    }

    #[tokio::test]
    async fn test_insert_returns_new_id_without_image() {
        let pool = setup_memory_pool().await;
        let existing = insert_post(&pool, "既存").await;

        let id = insert(&pool, "タイトル", "本文").await.unwrap();

        assert!(id > existing, "新しい id が採番されること");
        let post = find(&pool, id).await.unwrap().expect("投稿が見つかること");
        assert_eq!(post.title, "タイトル");
        assert_eq!(post.text, "本文");
        assert_eq!(post.image, None);
        assert_eq!(post.regist_date, post.update_date);
    }

    #[tokio::test]
    async fn test_update_image_path_sets_and_clears_image() {
        let pool = setup_memory_pool().await;
        let id = insert(&pool, "タイトル", "本文").await.unwrap();

        update_image_path(&pool, id, Some("1.png")).await.unwrap();
        assert_eq!(
            find(&pool, id).await.unwrap().unwrap().image.as_deref(),
            Some("1.png")
        );

        update_image_path(&pool, id, None).await.unwrap();
        assert_eq!(find(&pool, id).await.unwrap().unwrap().image, None);
    }

    #[tokio::test]
    async fn test_update_changes_title_and_text_but_not_image() {
        let pool = setup_memory_pool().await;
        let id = insert(&pool, "古いタイトル", "古い本文").await.unwrap();
        let other = insert(&pool, "別の投稿", "そのまま").await.unwrap();
        update_image_path(&pool, id, Some("1.png")).await.unwrap();
        // datetime('now') は秒単位なので、更新されたと分かるよう日時を過去にずらしておく
        sqlx::query("UPDATE post SET update_date = '2000-01-01 00:00:00'")
            .execute(&pool)
            .await
            .unwrap();

        update(&pool, id, "新しいタイトル", "新しい本文")
            .await
            .unwrap();

        let post = find(&pool, id).await.unwrap().unwrap();
        assert_eq!(post.title, "新しいタイトル");
        assert_eq!(post.text, "新しい本文");
        assert_eq!(post.image.as_deref(), Some("1.png"));
        assert_ne!(post.update_date.to_string(), "2000-01-01 00:00:00");
        assert_eq!(find(&pool, other).await.unwrap().unwrap().title, "別の投稿");
    }

    #[tokio::test]
    async fn test_delete_removes_only_that_post() {
        let pool = setup_memory_pool().await;
        let keep = insert_post(&pool, "残す").await;
        let gone = insert_post(&pool, "消す").await;

        delete(&pool, gone).await.unwrap();

        assert!(find(&pool, gone).await.unwrap().is_none());
        assert!(find(&pool, keep).await.unwrap().is_some());
    }
}
