-- 投稿テーブル（post）の初期マイグレーション
--
-- ファイル名規約: {VERSION}_{description}.sql （例: 0001_init.sql, 0002_add_xxx.sql）
-- sqlx は先頭の VERSION 部分を整数として解釈し、昇順に適用する。
-- そのため "0010_xxx.sql" と "0002_xxx.sql" を並べた場合、文字列順とは違って
-- 0002 → 0010 の順で正しく適用される。
--
-- 一度適用したマイグレーションは sqlx が自動生成する _sqlx_migrations テーブルに
-- VERSION と SQL のチェックサム付きで記録され、次回起動時はスキップされる（冪等）。
-- 既に適用済みの SQL を後から書き換えると checksum mismatch エラーになるので、
-- 適用済みマイグレーションは触らず、新しい連番ファイルで変更を追加するのが鉄則。

-- 投稿 (post)
-- 旧PHP `post` テーブル・要件 REQ-001/REQ-002/EDGE-101〜104 をもとに定義。
CREATE TABLE IF NOT EXISTS post (
    -- 投稿ID。SQLite では `INTEGER PRIMARY KEY` で AUTOINCREMENT 同等になる（rowid alias）
    -- AUTOINCREMENT を明示することで、削除済み id を再利用しないことを保証する
    id          INTEGER PRIMARY KEY AUTOINCREMENT,

    -- タイトル（必須、1〜50文字）
    -- SQLite の TEXT 型は長さ上限がないため、CHECK 制約で文字数を明示的に制限する
    title       TEXT NOT NULL,

    -- 本文（任意、0〜500文字。空文字を許容するため NOT NULL DEFAULT '' とする）
    -- NULL と空文字を区別しないことで、アプリ側の NULL チェックを不要にする
    text        TEXT NOT NULL DEFAULT '',

    -- 画像の相対パス（例: "uploads/{id}.{ext}"）。画像なし投稿を許容するため NULL 可
    image       TEXT,

    -- 登録日時（SQLite の datetime() 形式の文字列で保存。例: "2026-05-22 14:30:00"）
    -- SQLite には専用の日時型がないため TEXT で扱う。
    -- 注意: datetime('now') は UTC で記録される。ローカル時刻が欲しい場合は
    -- datetime('now','localtime') を使うか、表示時にアプリ側でタイムゾーン変換する
    regist_date TEXT NOT NULL DEFAULT (datetime('now')),

    -- 更新日時。UPDATE 時にアプリ側で datetime('now') を渡して上書きする
    -- regist_date と同様 UTC 保存
    update_date TEXT NOT NULL DEFAULT (datetime('now')),

    -- 制約: title は 1〜50文字（空文字・長すぎる投稿を DB レベルで拒否する）
    CONSTRAINT post_title_len_chk CHECK (length(title) BETWEEN 1 AND 50),

    -- 制約: text は 0〜500文字（500文字を超える本文を DB レベルで拒否する）
    CONSTRAINT post_text_len_chk CHECK (length(text) <= 500)
);
