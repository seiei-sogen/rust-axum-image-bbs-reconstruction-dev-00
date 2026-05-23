# rust-axum-image-bbs 設計ヒアリング記録

**作成日**: 2026-05-22
**ヒアリング実施**: step4 既存情報ベースの差分ヒアリング

## ヒアリング目的

要件定義フェーズ（[../spec/](../spec/)）で技術スタック（axum / sqlx / SQLite / Askama / tracing）は確定済みだが、設計レベルの選択肢（URL設計・エラー型・テンプレ構成・トランザクション戦略など）が未確定だった。
それらを差分ヒアリングで確定させ、信頼性レベル 🟡 だった項目を 🔵 に引き上げる目的で実施。

## 質問と回答

### Q1: 既存実装（旧PHPコード）の詳細分析は必要か

**質問日時**: 2026-05-22
**カテゴリ**: コード分析方針
**背景**: 要件定義フェーズで旧PHPの主要ファイル（index.php / admin_*.php / DBImgbbs*.php）は読了済み。設計を進める前にもう一度網羅的調査が必要か確認。

**回答**: 不要（推奨）

**信頼性への影響**:
- 既存読解で十分という判断が得られたため、Rust 設計に集中。設計時間を確保。

---

### Q2: URL 設計の方針

**質問日時**: 2026-05-22
**カテゴリ**: アーキテクチャ
**背景**: 旧PHPは画面ごとにフラットな .php ファイル名（admin_edit.php?id=N）だった。Rust では axum のリソース指向ルーティングを採用するかが未確定。

**回答**: RESTful・リソース型（推奨）
- `GET /` (一覧)
- `POST /posts` (作成)
- `GET /posts/{id}/edit` (編集画面)
- `POST /posts/{id}/edit` (編集処理)
- `POST /posts/{id}/delete` (削除処理)

**信頼性への影響**:
- api-endpoints.md の全エンドポイント定義（🔵）が確定
- 旧PHPからの URL 変更が明確化（旧 `admin_delete.php?id=N` GET → 新 `POST /posts/:id/delete`）

---

### Q3: エラー型の設計方針

**質問日時**: 2026-05-22
**カテゴリ**: エラーハンドリング戦略
**背景**: Rust では `anyhow`（簡易）か `thiserror`（ドメインエラー型）か、または両方かの選択がある。学習価値と複雑度のバランス確認。

**回答**: thiserror + axum IntoResponse 実装（推奨）

**信頼性への影響**:
- `errors.rs` の `AppError` 定義（🔵）が確定
- `interfaces.rs` の `AppError` バリアント（Validation / NotFound / PayloadTooLarge / Io / Sqlx / Other）が全て 🔵
- dataflow.md のエラーハンドリングフロー図が 🔵

---

### Q4: Cargo ワークスペース構成

**質問日時**: 2026-05-22
**カテゴリ**: プロジェクト構造
**背景**: 大規模化を見据えると workspace 分割（domain/web/infra）も選択肢。学習用途として複雑さが釣り合うか確認。

**回答**: 単一クレート（推奨）

**信頼性への影響**:
- architecture.md のディレクトリ構造（🔵）が確定。`src/` 下のモジュラ構成で進める。
- Cargo.toml が 1 つだけになるため Zenn 記事でも説明しやすい

---

### Q5: テンプレート（Askama）の構成

**質問日時**: 2026-05-22
**カテゴリ**: UI/ビュー層設計
**背景**: Askama は `{% extends %}` をサポート。共通レイアウト抽出するか、各ファイルを独立させるか。

**回答**: base.html + extends パターン（推奨）

**信頼性への影響**:
- architecture.md の `templates/` 構成が 🔵 で確定（base.html, index.html, edit.html）
- Bootstrap CDN リンクやレスポンシブ meta タグを base.html に集約できる

---

### Q6: DB マイグレーションの運用方針

**質問日時**: 2026-05-22
**カテゴリ**: データモデル / 運用
**背景**: sqlx は CLI（手動）と `sqlx::migrate!()` マクロ（起動時自動）の2方式がある。学習・Docker 起動利便性とのバランス。

**回答**: sqlx::migrate!() を起動時自動実行（推奨）

**信頼性への影響**:
- database-schema.sql の運用方針セクションが 🔵 で確定
- Docker でも初回起動でテーブルが自動作成されるので docker-compose の体験がよい

---

### Q7: リクエストヘッダー・トレーシング設計

**質問日時**: 2026-05-22
**カテゴリ**: 観測可能性
**背景**: NFR-402（tracing 構造化ログ）の具体実装。tower-http の TraceLayer を使うか手書きするか。

**回答**: tower-http::trace::TraceLayer 使用（推奨）

**信頼性への影響**:
- architecture.md の TraceLayer ミドルウェアが 🔵
- request_id（span）の自動付与により、ハンドラ内のログにリクエスト ID が紐づく

---

### Q8: AppState に含める内容

**質問日時**: 2026-05-22
**カテゴリ**: 状態管理 / 設定
**背景**: axum の State Extractor 経由で共有する状態の組み立て。

**回答**:
- DB接続プール（SqlitePool）✅
- アップロードディレクトリのパス（UploadDir）✅
- システム設定（ページサイズ・画像上限等）✅
- ※ Askama はコンパイル時テンプレートなので AppState には不要

**信頼性への影響**:
- interfaces.rs の `AppState` / `AppConfig` 構造体（🔵）が確定
- 環境変数（DATABASE_URL / UPLOAD_DIR / PORT 等、NFR-403）と AppState の対応が明確化

---

### Q9: 画像保存時のトランザクション戦略

**質問日時**: 2026-05-22
**カテゴリ**: データ整合性
**背景**: EDGE-005（画像保存失敗時のロールバック）を満たす実装方針。3パターン（INSERT-保存-UPDATE / 保存-INSERT / 後決め）から選択。

**回答**: INSERT → ファイル保存 → UPDATE の3ステップ（推奨）

**信頼性への影響**:
- dataflow.md の Create シーケンス図（🔵）が確定
- 旧PHP `DBImgbbs::InsertImgbbs` + `UpdateInsertImgbbs` のロジックを踏襲しつつ、失敗時のロールバック（補償 DELETE）を明文化

---

### Q10: アップロードされた画像の配信方法

**質問日時**: 2026-05-22
**カテゴリ**: 静的配信 / セキュリティ
**背景**: ハンドラ自前でファイル配信するか、専用ミドルウェアを使うか。

**回答**: tower-http::ServeDir で静的ファイル配信（推奨）

**信頼性への影響**:
- api-endpoints.md の `GET /uploads/*` セクション（🔵）が確定
- ServeDir 自身のパストラバーサル対策により NFR-103 の二重防御になる

---

### Q11: Bootstrap の配信方法

**質問日時**: 2026-05-22
**カテゴリ**: フロントエンド配信
**背景**: 旧PHPは `css/bootstrap.min.css` を同梱していた。Rust 版もファイル同梱か CDN か。

**回答**: CDN 経由（推奨）

**信頼性への影響**:
- base.html の `<link>` タグは CDN（jsdelivr 等）を指す
- Docker イメージサイズが小さくなる
- オフライン開発時はCDN到達不可のためコメントで注意喚起する余地（🟡）

---

### Q12: 実装フェーズ計画

**質問日時**: 2026-05-22
**カテゴリ**: スコープ・タスク分割
**背景**: Zenn 記事化を前提に、設計レベルでフェーズを切るかどうか。kairo-tasks に任せるか。

**回答**: Phase 1-3 に分ける（推奨）

**信頼性への影響**:
- architecture.md の「実装フェーズ計画」セクションが 🔵 で確定
- Phase 1: 基盤（起動・DB・一覧・Create）
- Phase 2: 編集・削除・画像限定処理・エラーハンドリング
- Phase 3: ページング・テスト・Docker・ログ仕上げ

---

## ヒアリング結果サマリー

### 確認できた事項

- **アーキテクチャ**: レイヤードモジュラモノリス（単一クレート、`src/` 下に handlers/models/repository/storage/validators/templates）
- **URL**: RESTful リソース指向
- **エラー処理**: thiserror で AppError 列挙、IntoResponse でHTTP変換
- **テンプレート**: Askama + base.html extends パターン、CDN Bootstrap
- **DB**: SQLite + sqlx::migrate!() 起動時自動
- **状態**: AppState（pool + upload_dir + config）を Arc 共有
- **画像TX**: INSERT → save → UPDATE、失敗時 DELETE で補償
- **配信**: tower-http ServeDir で /uploads/* 静的配信
- **観測**: tower-http TraceLayer
- **フェーズ**: Phase 1-3 構成

### 設計方針の決定事項

1. axum 0.7 + sqlx 0.8 + Askama 0.12 を採用クレートのコア
2. Cargo workspace は使わず、単一クレート + モジュール分割
3. ハンドラはすべて `async fn handler(State<AppState>, ...) -> AppResult<impl IntoResponse>` 形式
4. 旧PHPからの主な改善点:
   - 削除 GET → POST
   - 画像のみ削除（チェックボックス）を正式実装
   - 拡張子変更時の旧ファイル削除を明示
   - 入力値保持
   - tracing 構造化ログ

### 残課題

- **CSRF 対策**: 認証なしアプリだが、POSTエンドポイントへの保護をどう扱うか（prep.md の確認事項として残存、🟡）
- **画像差し替えと delete_image=on の同時送信時の挙動**: 「画像優先」と仮置きしたが、UI で排他にする方が安全（🟡）
- **PRAGMA journal_mode=WAL**: パフォーマンスのため有効化を検討（学習として記事化価値あり、🟡）
- **静的画像のキャッシュヘッダ**: ServeDir のデフォルトで十分か、Cache-Control を明示するか（学習対象として価値あり、🟡）

### 信頼性レベル分布

**ヒアリング前**（要件定義のみで設計開始しようとした時点）:
- 🔵 青信号: 約15項目（要件定義済みの技術スタックのみ）
- 🟡 黄信号: 約30項目（設計判断未確定の項目すべて）
- 🔴 赤信号: 数項目（エラー型・テンプレ構成等の推測になる項目）

**ヒアリング後**:
- 🔵 青信号: 約66項目 (+51、大幅増)
- 🟡 黄信号: 約17項目（残課題と妥当な推測の範囲）
- 🔴 赤信号: 約3項目（「該当なし」を明示した部分）

## 関連文書

- **アーキテクチャ設計**: [architecture.md](architecture.md)
- **データフロー**: [dataflow.md](dataflow.md)
- **型定義**: [interfaces.rs](interfaces.rs)
- **DBスキーマ**: [database-schema.sql](database-schema.sql)
- **エンドポイント仕様**: [api-endpoints.md](api-endpoints.md)
- **要件定義**: [../spec/requirements.md](../spec/requirements.md)
