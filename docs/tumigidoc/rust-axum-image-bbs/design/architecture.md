# rust-axum-image-bbs アーキテクチャ設計

**作成日**: 2026-05-22
**関連要件定義**: [../spec/requirements.md](../spec/requirements.md)
**ヒアリング記録**: [design-interview.md](design-interview.md)

**【信頼性レベル凡例】**:
- 🔵 **青信号**: EARS要件定義書・ユーザヒアリングを参考にした確実な設計
- 🟡 **黄信号**: EARS要件定義書・ユーザヒアリングから妥当な推測による設計
- 🔴 **赤信号**: EARS要件定義書・ユーザヒアリングにない推測による設計

---

## システム概要 🔵

**信頼性**: 🔵 *要件定義書「概要」より*

Rust初心者の学習用に、旧PHP画像掲示板を Rust + axum で再実装するサーバサイドレンダリング型のWebアプリケーション。
ユーザーが画像・タイトル・本文を投稿し、一覧表示・編集・削除ができる単一画面構成のCRUDアプリ。
ログイン・管理者機能は対象外（要件 REQ-405 / REQ-406）。実装プロセスはZenn記事化を前提に、ステップごとに区切って進める。

## アーキテクチャパターン 🔵

**信頼性**: 🔵 *ヒアリング「単一クレート」「RESTful URL」より*

- **パターン**: **レイヤードモジュラモノリス（単一クレート）**
- **選択理由**:
  - 学習目的で全体を見通しやすい単一バイナリ構成（ヒアリング「単一クレート」採用）
  - axum のルーター → handlers → domain → infra(db/storage) の素直なレイヤ
  - クリーンアーキテクチャや DDD まで踏み込むと Rust 初学者には認知負荷が高い
- **責務分離**:
  - `handlers/`: HTTP リクエスト/レスポンス変換とフォームバリデーション
  - `models/` (or `domain/`): 投稿エンティティとビジネスロジック
  - `repository/`: sqlx を使ったDBアクセス（リポジトリパターン）
  - `storage/`: 画像ファイルの保存・削除
  - `templates/` (Askama): HTML描画
  - `errors.rs`: アプリ共通のエラー型と HTTP 変換

## コンポーネント構成

### Webフレームワーク層 🔵

**信頼性**: 🔵 *要件 REQ-401・ヒアリングより*

- **フレームワーク**: **axum 0.7系**
- **ルーター**: `axum::Router` で `/`、`/posts`、`/posts/{id}/edit`、`/posts/{id}/delete` を定義
- **ミドルウェア**:
  - `tower-http::trace::TraceLayer`（リクエスト/レスポンスログ）🔵 *ヒアリング*
  - `tower-http::services::ServeDir`（`/uploads/*` の静的配信）🔵 *ヒアリング*
  - `axum::extract::DefaultBodyLimit` で 5MB 上限 🔵 *EDGE-001*
- **抽出 (Extractor)**:
  - `axum::extract::State<AppState>` で共有状態
  - `axum::extract::Path<i64>` で `id`
  - `axum::extract::Query<PaginationQuery>` で `?page=N`
  - `axum::extract::Multipart` で画像付きフォーム受け取り

### テンプレート層 🔵

**信頼性**: 🔵 *要件 REQ-403・ヒアリング「Askama / base.html + extends」より*

- **エンジン**: **Askama 0.12系**（コンパイル時テンプレート、自動エスケープ）
- **テンプレート構成**:
  ```
  templates/
  ├── base.html          # 共通レイアウト（HTMLヘッド、Bootstrap CDN、コンテナ）
  ├── index.html         # 投稿一覧 + 投稿フォーム（`extends "base.html"`）
  └── edit.html          # 編集フォーム（`extends "base.html"`）
  ```
- **自動エスケープ**: Askama デフォルトの `escape="html"` を使用し XSS を防止 🔵 *NFR-104*
- **構造体パターン**: `#[derive(Template)] #[template(path = "index.html")] struct IndexTemplate { posts: Vec<Post>, page: u32, total_pages: u32, ... }`

### ドメイン層 🟡

**信頼性**: 🟡 *要件と慣用パターンから妥当な推測*

- **エンティティ**: `Post`（後述 interfaces.rs 参照）
- **バリデーション**: `validator` クレート、もしくはハンドコードした関数 `validate_title()` / `validate_text()` / `validate_image()`
- **画像MIME判定**: `infer` クレートでマジックナンバー判定 🔵 *NFR-102*

### データベース層 🔵

**信頼性**: 🔵 *要件 REQ-402・ヒアリングより*

- **DBMS**: **SQLite 3**（ファイル: `data/app.db`）
- **クライアント**: **sqlx 0.8系**
  - features: `runtime-tokio`、`sqlite`、`macros`、`chrono`、`migrate`
  - クエリは `sqlx::query!` / `sqlx::query_as!` マクロを使用（コンパイル時SQLチェック）🔵 *NFR-101*
- **接続プール**: `SqlitePoolOptions::new().max_connections(5)` 程度（学習用途）
- **マイグレーション**: `sqlx::migrate!("./migrations").run(&pool).await?` を起動時に実行 🔵 *ヒアリング*

### ストレージ層 🔵

**信頼性**: 🔵 *要件 REQ-404・ヒアリングより*

- **画像保存先**: `<UploadDir>/{id}.{ext}` （デフォルト `uploads/`）
- **配信**: `tower-http::services::ServeDir::new(&upload_dir)` を `/uploads` にネスト
- **書き込み**: `tokio::fs::write(path, bytes).await?` で非同期書き込み

### 共有状態（AppState） 🔵

**信頼性**: 🔵 *ヒアリング「DBプール・UploadDir・Config」より*

```rust
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,           // 🔵 ヒアリング採択
    pub upload_dir: PathBuf,      // 🔵 ヒアリング採択
    pub config: AppConfig,        // 🔵 ヒアリング採択
}

#[derive(Clone)]
pub struct AppConfig {
    pub page_size: u32,           // 🔵 REQ-012 (20件/ページ)
    pub max_image_bytes: usize,   // 🔵 EDGE-001 (5MB)
    pub allowed_mime: &'static [&'static str], // 🔵 EDGE-002
}
```

- `Arc<AppState>` を `Router::with_state()` 経由で共有
- `Clone` を派生させることで axum の State Extractor から取り出し可能

## システム構成図 🔵

**信頼性**: 🔵 *要件・ヒアリングより*

```mermaid
graph TB
    Browser[ブラウザ]

    subgraph Axum["axum サーバ (単一バイナリ)"]
        Trace[TraceLayer<br/>tower-http]
        Router[axum::Router]
        ServeDir[ServeDir<br/>/uploads/*]

        subgraph Handlers["handlers/"]
            H_List[GET /<br/>list]
            H_New[POST /posts<br/>create]
            H_Edit[GET/POST<br/>/posts/:id/edit]
            H_Delete[POST<br/>/posts/:id/delete]
        end

        subgraph Domain["models/ + validators"]
            Post[Post Entity]
            Validate[validate_*]
        end

        subgraph Infra["infra layer"]
            Repo[repository::PostRepo<br/>sqlx queries]
            Storage[storage::save/delete<br/>tokio::fs]
        end

        Templates[Askama Templates<br/>base/index/edit]
        Errors[AppError<br/>IntoResponse]
    end

    DB[(SQLite<br/>data/app.db)]
    Files[(uploads/<br/>{id}.{ext})]

    Browser -->|HTTP| Trace
    Trace --> Router
    Router --> Handlers
    Router --> ServeDir
    ServeDir --> Files
    Handlers --> Domain
    Handlers --> Templates
    Handlers --> Errors
    Domain --> Validate
    Handlers --> Repo
    Handlers --> Storage
    Repo --> DB
    Storage --> Files
```

## ディレクトリ構造 🔵

**信頼性**: 🔵 *単一クレート・モジュラ設計のRust慣用パターンより*

```
./
├── Cargo.toml
├── Cargo.lock
├── .env.example                 # DATABASE_URL, UPLOAD_DIR, PORT 等のサンプル
├── Dockerfile
├── docker-compose.yml
├── data/                        # SQLite ファイル置き場（ボリュームマウント対象）
│   └── .gitkeep
├── uploads/                     # 画像保存先（ボリュームマウント対象）
│   └── .gitkeep
├── migrations/
│   └── 0001_init.sql            # post テーブル作成
├── templates/
│   ├── base.html
│   ├── index.html
│   └── edit.html
├── tests/
│   ├── common/mod.rs            # テスト用 setup ヘルパ
│   ├── integration_list.rs
│   ├── integration_create.rs
│   ├── integration_edit.rs
│   └── integration_delete.rs
├── src/
│   ├── main.rs                  # エントリポイント、ルーター組み立て、TraceLayer設定
│   ├── lib.rs                   # 統合テストから参照できるよう公開API集約
│   ├── config.rs                # AppConfig / 環境変数読み込み
│   ├── state.rs                 # AppState 定義
│   ├── errors.rs                # AppError + IntoResponse
│   ├── handlers/
│   │   ├── mod.rs
│   │   ├── posts.rs             # 一覧 / 作成 / 編集 / 削除
│   │   └── pagination.rs        # PaginationQuery extractor
│   ├── models/
│   │   ├── mod.rs
│   │   └── post.rs              # Post 構造体・FromRow
│   ├── repository/
│   │   ├── mod.rs
│   │   └── post_repo.rs         # sqlx クエリ集約
│   ├── storage/
│   │   ├── mod.rs
│   │   └── image_store.rs       # save_image / delete_image
│   ├── validators/
│   │   ├── mod.rs
│   │   └── post_form.rs         # title/text/image バリデーション
│   └── templates/
│       └── mod.rs               # Askama struct 定義（index/edit）
└── docs/
    └── tumigidoc/rust-axum-image-bbs/...
```

## 非機能要件の実現方法

### パフォーマンス 🟡

**信頼性**: 🟡 *NFR-001・NFR-002から妥当な推測*

- **レスポンスタイム**: 100件規模で <500ms (NFR-001)
  - SQLite の WAL モード有効化 🟡
  - `idx_post_id_desc`（PRIMARY KEY なので id 降順スキャンは自然に効く）
- **スループット**: 同時 10 接続程度を想定（NFR-002）。`SqlitePool` の `max_connections=5` で十分
- **テンプレート**: Askama はコンパイル時にコード生成されるためランタイム解析コストなし

### セキュリティ 🔵

**信頼性**: 🔵 *NFR-101〜NFR-105 + 旧PHPとの差分より*

- **SQLインジェクション**: sqlx の `query!`/`query_as!` マクロでパラメータ化（NFR-101）🔵
- **画像MIME偽装**: `infer` クレートでマジックナンバー判定（NFR-102, EDGE-002）🔵
- **パストラバーサル**: ファイル名はユーザー入力を使わず `{id}.{許可拡張子}` で再構築（NFR-103）🔵
- **XSS**: Askama の自動 HTML エスケープを有効化（NFR-104）🔵
- **POST/GET 分離**: 削除・編集は POST のみ（NFR-105）🔵
- **DoS**: `axum::extract::DefaultBodyLimit::max(5 * 1024 * 1024)` で 5MB に制限 🔵 *EDGE-001*

### スケーラビリティ 🟡

**信頼性**: 🟡 *要件外だが念のため*

- 学習用途のためスケーリング戦略は対象外（要件 NFR-002）
- SQLite + ローカルファイル構成のため水平スケールは不可。本番デプロイ時に別途検討

### 可用性 🟡

**信頼性**: 🟡 *学習用途として最低限*

- ローカル起動のみで、SLA・監視は対象外
- 起動時のマイグレーション失敗は `panic!` で即時停止（運用者が気づける）🟡

### 学習・保守性 🔵

**信頼性**: 🔵 *REQ-407 / NFR-301〜NFR-303 より*

- 各モジュールの先頭に「このモジュールは何をするか」を日本語コメント
- 関数・型には `///` ドキュメンテーションコメントで所有権・`Result`・`async` を補足
- `cargo test` で単体・統合テストが一括実行可能

## 技術的制約

### パフォーマンス制約 🔵

**信頼性**: 🔵 *NFR-001・NFR-002より*

- SQLite ファイル1つ・単一プロセスでの動作前提（同時接続数 ~10）
- 画像配信はサーバ自身が担う（CDN不使用）

### セキュリティ制約 🔵

**信頼性**: 🔵 *要件定義より*

- 認証なしのアプリ（REQ-405）のため、CSRF 対策は本要件では Optional（prep.md の確認事項）
- アップロード可能拡張子は jpg/gif/png のみ
- アップロードファイル最大 5MB

### 互換性制約 🔵

**信頼性**: 🔵 *ヒアリングより*

- Rust 1.75+（axum 0.7・Askama 0.12 のMSRV相当）
- ローカル + Docker 起動のみ想定。本番デプロイは対象外（NFR-401 のみ満たす）

## 主要な依存クレート（暫定） 🔵

**信頼性**: 🔵 *ヒアリング選定の素直な対応*

| クレート | バージョン目安 | 用途 |
|---|---|---|
| `axum` | 0.7 | Webフレームワーク |
| `tokio` | 1.x (full) | 非同期ランタイム |
| `tower-http` | 0.5 | TraceLayer / ServeDir |
| `sqlx` | 0.8 | DB（sqlite/macros/migrate） |
| `askama` | 0.12 | テンプレート |
| `askama_axum` | 0.4 | axum 連携（IntoResponse） |
| `serde` | 1.x | derive 用 |
| `tracing` / `tracing-subscriber` | 0.1 / 0.3 | ログ |
| `thiserror` | 1.x | エラー型定義 |
| `anyhow` | 1.x | テスト・初期化エラー |
| `infer` | 0.16 | 画像MIME判定 |
| `chrono` | 0.4 | 日時 |
| `dotenvy` | 0.15 | .env 読み込み |
| `uuid` | (任意) | 今回はid連番で済むため利用しない可能性大 |

## 実装フェーズ計画 🔵

**信頼性**: 🔵 *ヒアリング「Phase 1-3」より*

### Phase 1: 基盤構築（Zenn 5〜7記事相当）
1. Cargo init + axum hello world
2. ルーティング + ServeDir
3. sqlx + SQLite 接続 + マイグレーション
4. Askama テンプレート（base + index）
5. 一覧表示（Read）
6. 投稿作成（Create + 画像アップロード）

### Phase 2: 編集・削除（Zenn 3〜4記事相当）
7. 編集画面表示（GET）
8. 編集処理（POST、画像差し替え対応）
9. 削除処理（POST、画像ファイル連動）
10. バリデーション統合・エラーハンドリング（thiserror + IntoResponse）

### Phase 3: 仕上げ（Zenn 3〜4記事相当）
11. ページネーション
12. 単体テスト + 統合テスト
13. Dockerfile + docker-compose
14. tracing 構造化ログ仕上げ

## 関連文書

- **データフロー**: [dataflow.md](dataflow.md)
- **型定義**: [interfaces.rs](interfaces.rs)
- **DBスキーマ**: [database-schema.sql](database-schema.sql)
- **エンドポイント仕様**: [api-endpoints.md](api-endpoints.md)
- **要件定義**: [../spec/requirements.md](../spec/requirements.md)
- **ヒアリング**: [design-interview.md](design-interview.md)

## 信頼性レベルサマリー

- 🔵 青信号: 26件 (約79%)
- 🟡 黄信号: 7件 (約21%)
- 🔴 赤信号: 0件 (0%)

**品質評価**: 高品質
