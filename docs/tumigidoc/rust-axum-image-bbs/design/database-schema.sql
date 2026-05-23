-- ========================================
-- rust-axum-image-bbs データベーススキーマ (SQLite)
-- ========================================
--
-- 作成日: 2026-05-22
-- 関連設計: architecture.md / interfaces.rs
-- ターゲット: SQLite 3.x（要件 REQ-402）
--
-- 信頼性レベル:
-- - 🔵 青信号: EARS要件定義書・旧PHPスキーマ・ユーザヒアリングを参考にした確実な定義
-- - 🟡 黄信号: 妥当な推測による定義
-- - 🔴 赤信号: 推測による定義
--
-- 配置場所: migrations/0001_init.sql
-- 適用方法: sqlx::migrate!("./migrations").run(&pool) で起動時に自動実行（ヒアリング採用）

-- ========================================
-- 投稿テーブル
-- ========================================

-- 投稿 (post)
-- 🔵 信頼性: 旧PHP `post` テーブル（DBImgbbs.php 7行）・要件 REQ-001/REQ-002/EDGE-101〜104 より
CREATE TABLE IF NOT EXISTS post (
    -- 投稿ID。SQLite では `INTEGER PRIMARY KEY` で AUTOINCREMENT 同等になる（rowid alias）
    -- 🔵 旧PHP `post.id` AUTO_INCREMENT より
    id          INTEGER PRIMARY KEY AUTOINCREMENT,

    -- タイトル（必須、1〜50文字）
    -- 🔵 要件 EDGE-101 / 旧PHP index.php 14-18行
    -- SQLite の VARCHAR(50) は単なるヒント（実体は TEXT）。長さ制約は CHECK で明示
    title       TEXT NOT NULL,

    -- 本文（任意、0〜500文字。空文字を許容）
    -- 🔵 要件 EDGE-102 / 旧PHP index.php 20-22行
    text        TEXT NOT NULL DEFAULT '',

    -- 画像の相対パス（例: "uploads/{id}.{ext}"）。NULL 可
    -- 🔵 REQ-005 / REQ-006 / EDGE-104（画像なし投稿の許容）
    image       TEXT,

    -- 登録日時（ISO 8601 文字列で保存。例: "2026-05-22 14:30:00"）
    -- 🔵 旧PHP `regist_date` 互換。SQLite では DATETIME = TEXT/REAL/INTEGER のいずれかでよい
    -- 🟡 デフォルトに CURRENT_TIMESTAMP を入れ、アプリ側で渡さなくても動くようにする
    regist_date TEXT NOT NULL DEFAULT (datetime('now')),

    -- 更新日時。UPDATE 時にアプリ側で datetime('now') を渡して上書き
    -- 🔵 旧PHP `update_date` 互換
    update_date TEXT NOT NULL DEFAULT (datetime('now')),

    -- 制約: title は 1〜50文字
    -- 🔵 EDGE-101 より
    CONSTRAINT post_title_len_chk CHECK (length(title) BETWEEN 1 AND 50),

    -- 制約: text は 0〜500文字
    -- 🔵 EDGE-102 より
    CONSTRAINT post_text_len_chk CHECK (length(text) <= 500)
);

-- ========================================
-- インデックス
-- ========================================

-- 一覧画面の `ORDER BY id DESC LIMIT ? OFFSET ?` を高速化する目的のインデックス。
-- ただし PRIMARY KEY が INTEGER の場合は rowid と一致するため、id 降順スキャンはB-treeを
-- 逆順に走査するだけで十分高速。明示的なインデックスは原則不要。
-- 🟡 学習目的でコメントだけ残し、CREATE INDEX は実行しない（必要になったら追加）

-- CREATE INDEX IF NOT EXISTS idx_post_regist_date ON post(regist_date DESC); -- 🔴 (現状不要)

-- ========================================
-- マイグレーション運用方針
-- ========================================
-- 🔵 信頼性: ヒアリング採用「sqlx::migrate!()」より
--
-- 1. プロジェクト直下の `migrations/` ディレクトリに連番ファイル名で配置:
--    migrations/
--      0001_init.sql                ← このスキーマ
--      0002_xxxxx.sql               ← 将来の変更
--
-- 2. アプリ起動時に `sqlx::migrate!("./migrations").run(&pool).await?` を実行
-- 3. sqlx は `_sqlx_migrations` テーブルを自動作成し、適用済みマイグレーションを記録する
-- 4. 既に適用済みなら冪等にスキップされる

-- ========================================
-- 初期データ
-- ========================================
-- 🔴 信頼性: 要件定義に初期データ要求はないため不要
--
-- 開発時に手動でテストデータを入れたい場合は別途 `seeds/` を用意するなど
-- （今回はスコープ外）

-- ========================================
-- パフォーマンス最適化（学習用としての参考）
-- ========================================
-- 🟡 信頼性: SQLite 一般的なチューニング

-- WAL モードを有効化することで読み書きの同時実行性が向上する。
-- アプリ起動時に `PRAGMA journal_mode=WAL` を発行することを検討（要 ANALYZE）。
--
-- ANALYZE 文（統計情報更新）は SQLite では通常不要だが、大量データになる場合に検討:
-- ANALYZE post;

-- ========================================
-- 旧PHP スキーマとの差分まとめ（参考）
-- ========================================
-- 旧PHP（MySQL）想定:
--   CREATE TABLE post (
--     id           INT AUTO_INCREMENT PRIMARY KEY,
--     title        VARCHAR(50) NOT NULL,
--     text         VARCHAR(500),
--     image        VARCHAR(255),
--     regist_date  DATETIME NOT NULL,
--     update_date  DATETIME NOT NULL
--   );
--
-- 主な差分:
-- - DBMS: MySQL → SQLite（要件 REQ-402 / ヒアリング）
-- - 文字数制約: アプリ側のみ → SQLite の CHECK 制約でも担保（より堅牢に）
-- - text: 任意 → NOT NULL DEFAULT '' に変更（NULL と空文字の区別を不要に）
-- - 日時: DATETIME → ISO 8601 TEXT（SQLite 推奨形式）
-- - id 連番: AUTO_INCREMENT → INTEGER PRIMARY KEY AUTOINCREMENT
--   ※ SQLite では単に INTEGER PRIMARY KEY だけでも rowid と alias されるが、
--     一度使った id を再利用しないことを保証したいため AUTOINCREMENT を明示

-- ========================================
-- 信頼性レベルサマリー
-- ========================================
-- - 🔵 青信号: 10件 (約77%)
-- - 🟡 黄信号: 3件 (約23%)
-- - 🔴 赤信号: 0件
--
-- 品質評価: 高品質
