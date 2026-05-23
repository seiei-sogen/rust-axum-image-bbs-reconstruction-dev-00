# rust-axum-image-bbs コンテキストノート

**作成日**: 2026-05-22
**プロジェクト**: rust-axum-image-bbs-reconstruction-dev-00

## プロジェクト概要

- **動機・目的**: Rust初心者が学習目的で、昔作ったPHP画像掲示板をRust + axumにリプレイスする
- **学習・発表**: 実装過程をZennでステップごとに記事化する
- **コメント方針**: ソースコード内のコメントは初学者向けに日本語で丁寧に記述する

## 旧PHP実装のサマリー

参照元: `/home/agir_wsl/develop/ghq_repos/github.com/seiei-sogen/rust-axum-qwik-imgboard-reconstruction/old_php_img_bbs/`

### ファイル構成（主要なもの）

| ファイル | 役割 |
|---|---|
| `index.php` | ユーザー画面：投稿フォーム + 投稿一覧（Read + Create） |
| `DBImgbbs.php` | ユーザー画面用のDB操作（Insert/UpdateImage/SelectAll） |
| `admin_list.php` | 管理画面：全投稿一覧 + 各行に編集・削除ボタン |
| `admin_edit.php` | 管理画面：投稿の編集フォーム |
| `admin_delete.php` | 管理画面：投稿の削除処理 |
| `DBImgbbsAdmin.php` | 管理画面用のDB操作（Update/Delete/個別Select） |
| `db.php` | MySQL PDO 接続クラス |
| `img/uploads/` | アップロードされた画像の保存先 |

### テーブル定義（推測）

```sql
CREATE TABLE post (
  id           INT AUTO_INCREMENT PRIMARY KEY,
  title        VARCHAR(50)  NOT NULL,
  text         VARCHAR(500),
  image        VARCHAR(255),         -- 画像ファイルへの相対パス
  regist_date  DATETIME NOT NULL,
  update_date  DATETIME NOT NULL
);
```

### 画像保存ルール

- パス: `img/uploads/{id}.{ext}` （拡張子は元ファイルから取得）
- 対応形式: `image/jpeg`、`image/gif`、`image/png` （MIME判定）

### バリデーション規則

- title: 必須、最大50文字
- text: 任意、最大500文字
- image: 任意、jpg/gif/png のみ

## Rustリプレイスでの技術選定（ヒアリング結果）

| カテゴリ | 採用 | 備考 |
|---|---|---|
| Webフレームワーク | **axum** | shiyou.md指定 |
| DB | **SQLite** | 学習用に1ファイル完結。MySQLとは異なる選択 |
| DBクライアント | **sqlx** | コンパイル時SQLチェック・async |
| テンプレート | **Askama** | コンパイル時テンプレート、型安全 |
| 画像保存 | **ローカルファイル** | 旧PHPと同方針 |
| 画像サイズ上限 | **5MB** | 旧PHPには明示的な上限なし |
| ページング | **あり（例: 20件/ページ）** | 旧PHPは全件表示だったが学習として追加 |
| ログ | **tracing + tracing-subscriber** | Rustエコシステム標準 |
| テスト | **cargo test（単体・統合）** | |
| Docker | **対応** | Dockerfile / docker-compose |
| UI | **Bootstrap（旧PHPと同等）** | UIに時間をかけず、Rust学習に集中 |
| コメント | **初学者向け・日本語・詳細** | 所有権、Result型などRust特有を丁寧に説明 |

## スコープ判断

### 含める（Must Have）

- 投稿一覧（画像・タイトル・本文・日付）
- 投稿作成（タイトル・本文・画像アップロード）
- 投稿編集（タイトル・本文・画像差し替え）
- 投稿削除（確認ダイアログ付き）
- ページング

### 含めない（一旦不要 / Won't Have）

- ユーザー認証・ログイン（shiyou.mdで「一旦なくてよい」）
- 管理者画面・権限制御（shiyou.mdで「一旦なくてよい」）
- コメント機能、タグ機能（今回のスコープ外）
- 本番デプロイ（ローカル + Docker のみ）

## 学習ガイドライン

- 各ステップごとにZenn記事化できる粒度で進める
- 例: Hello world → ルーティング → DB接続 → テンプレート → CRUD個別 → 画像アップロード → テスト → Docker
- Rust初心者がつまずきやすい所有権・借用・`Result<T, E>` `?`演算子・`async/.await`・`Arc<AppState>` などはコメントで補足
