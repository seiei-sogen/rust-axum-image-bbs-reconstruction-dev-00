# rust-axum-image-bbs エンドポイント仕様

**作成日**: 2026-05-22
**関連設計**: [architecture.md](architecture.md)
**関連要件定義**: [../spec/requirements.md](../spec/requirements.md)

**【信頼性レベル凡例】**:
- 🔵 **青信号**: EARS要件定義書・ユーザヒアリングを参考にした確実な定義
- 🟡 **黄信号**: 妥当な推測による定義
- 🔴 **赤信号**: 推測による定義

---

## 注記: 本ドキュメントの位置づけ

本システムはサーバサイドレンダリング（Askama テンプレート）の Web アプリであり、
JSON API ではなく **HTML を返す HTTP エンドポイント** を提供する。
ヒアリングで「RESTful・リソース型 URL 設計」を採用したため、リソース指向で記述する。
将来 JSON API を同梱する場合は、本ドキュメントを base にして `/api/v1/posts` 等を拡張する想定。

---

## 共通仕様

### ベースURL 🔵

**信頼性**: 🔵 *ヒアリング「ローカル起動」より*

```
http://localhost:3000
```

- ポートは環境変数 `PORT` で上書き可能（NFR-403）

### 認証 🔵

**信頼性**: 🔵 *要件 REQ-405・REQ-406 より*

認証なし（ログイン・ユーザー機能はスコープ外）。
すべてのエンドポイントは未認証で誰でも操作可能。

### レスポンス形式 🔵

**信頼性**: 🔵 *ヒアリング「Askama」より*

`Content-Type: text/html; charset=utf-8` を返す（JSONは返さない）。

### エラーレスポンス共通フォーマット 🔵

**信頼性**: 🔵 *ヒアリング「thiserror + IntoResponse」より*

| ステータス | 表示 | 主な原因 |
|---|---|---|
| 200 OK | 通常画面 | 取得成功 |
| 303 See Other | リダイレクト | PRG パターン（投稿・編集・削除の成功後） |
| 400 Bad Request | エラー入りの再描画画面 | バリデーション失敗（REQ-201） |
| 404 Not Found | 簡素な 404 ページ | 存在しない id、未定義パス |
| 405 Method Not Allowed | axum 標準 | GET で DELETE 等 |
| 413 Payload Too Large | エラーメッセージ | 5MB 超過（EDGE-001） |
| 500 Internal Server Error | 簡素なエラーページ | DB/IO 失敗（EDGE-004/005） |

サーバ内部エラー（500系）は `tracing::error!` で構造化ログを出力（NFR-402）。

### CORS 🔴

**信頼性**: 🔴 *該当なし（同一オリジン前提）*

ローカルブラウザからの利用のみのため CORS 対応は不要。

---

## エンドポイント一覧

### 投稿（post）リソース

#### GET / 🔵

**信頼性**: 🔵 *REQ-001 / REQ-002 / REQ-012 / TC-001-* より*

**関連要件**: REQ-001, REQ-002, REQ-012, REQ-101

**説明**: 投稿一覧 + 投稿フォームを含むトップページを返す。

**クエリパラメータ**:
- `page` (optional, u32, default=1): 1始まりのページ番号
- 不正な page（負数・非数）はサーバ側で 1 にフォールバック 🟡

**レスポンス（成功）**:
- 200 OK, `text/html`
- `IndexView`（投稿一覧・ページ情報・空の投稿フォーム）を Askama でレンダリング

**エラー**:
- 500: DB接続失敗時

**信頼性メモ**: 旧PHP `index.php` 70-71 行（`SelectImgbbsAll`）を踏襲。

---

#### POST /posts 🔵

**信頼性**: 🔵 *REQ-003〜006 / REQ-201 / REQ-202 / EDGE-001〜005 より*

**関連要件**: REQ-003, REQ-004, REQ-005, REQ-006, REQ-201, REQ-202

**説明**: 新規投稿を作成する。`Content-Type: multipart/form-data` を必須とする。

**リクエスト**:
- `title` (string, required, 1〜50字)
- `text` (string, optional, 0〜500字)
- `image` (file, optional, jpg/gif/png, 5MB以下)

**正常系レスポンス**: 303 See Other → `Location: /`

**異常系レスポンス**:
| ステータス | 条件 | 関連 |
|---|---|---|
| 400 | バリデーションエラー（title空・長さ超過・本文長さ超過・MIME不正） | EDGE-001/002, EDGE-101/102 |
| 413 | bodyサイズが 5MB を超過 | EDGE-001 |
| 500 | 画像保存失敗 → ロールバック発動 | EDGE-005 |

**400 時の挙動**:
- HTML を `IndexView { form: 入力値, errors: [...] }` で再描画（入力値を保持、REQ-201）

**信頼性メモ**: 旧PHP `index.php` 9-68 行を Rust で再構築。

---

#### GET /posts/{id}/edit 🔵

**信頼性**: 🔵 *REQ-008 / TC-009-E01 より*

**関連要件**: REQ-008

**パスパラメータ**:
- `id` (i64): 投稿ID

**説明**: 編集フォーム画面を返す。既存の `title` / `text` / `image` を初期値表示。

**レスポンス（成功）**: 200 OK, `EditView { post }` を描画

**エラー**:
| ステータス | 条件 |
|---|---|
| 404 | id が存在しない（EDGE-003） |
| 500 | DB接続失敗 |

**信頼性メモ**: 旧PHP `admin_edit.php` 38-43 行に対応。

---

#### POST /posts/{id}/edit 🔵

**信頼性**: 🔵 *REQ-009 / REQ-103 / REQ-104 / TC-009-* より*

**関連要件**: REQ-009, REQ-103, REQ-104

**パスパラメータ**:
- `id` (i64): 投稿ID

**説明**: 既存投稿を更新する。`multipart/form-data` を必須とする。

**リクエスト**:
- `title` (string, required, 1〜50字)
- `text` (string, optional, 0〜500字)
- `image` (file, optional): 新しい画像を選択した場合のみ
- `delete_image` (checkbox, optional): ON のとき画像を削除（REQ-103）

**正常系レスポンス**: 303 See Other → `Location: /`

**画像の取り扱い**:
| 入力状態 | 動作 |
|---|---|
| image なし、delete_image=off | `image` 列は不変 |
| image あり | 旧拡張子の画像ファイルを削除し、`uploads/{id}.{新ext}` に上書き保存・`image` 列更新 |
| delete_image=on | `image` 列を NULL に更新し、対応ファイルを削除 |
| image あり かつ delete_image=on | image を優先（delete_image は無視） 🟡 |

**異常系レスポンス**:
| ステータス | 条件 | 関連 |
|---|---|---|
| 400 | バリデーションエラー → `EditView{ post, form: 入力値, errors }` で再描画 | REQ-201 |
| 404 | id が存在しない | EDGE-003 |
| 413 | bodyサイズ超過 | EDGE-001 |
| 500 | 画像保存失敗・DB失敗 | EDGE-005 |

**信頼性メモ**: 旧PHP `admin_edit.php` 8-37 行に対応。`delete_image` は新規要件（REQ-103）。

---

#### POST /posts/{id}/delete 🔵

**信頼性**: 🔵 *REQ-010 / REQ-011 / REQ-102 / NFR-105 / TC-010-* より*

**関連要件**: REQ-010, REQ-011, REQ-102, NFR-105

**パスパラメータ**:
- `id` (i64): 投稿ID

**説明**: 投稿を削除する。クライアント側で `confirm()` 確認後にのみ送信されることを想定（REQ-102）。

**リクエスト**: ボディなし（ヘッダのみ）

**正常系レスポンス**: 303 See Other → `Location: /`

**サーバ処理**:
1. `find(id)` で存在確認（なければ 404）
2. `image` がある場合は `tokio::fs::remove_file(...)` で物理削除（失敗は警告ログのみ）
3. `DELETE FROM post WHERE id = ?`

**異常系レスポンス**:
| ステータス | 条件 | 関連 |
|---|---|---|
| 404 | id が存在しない | EDGE-003 |
| 405 | GET でアクセス | NFR-105 / 旧PHPからの改善 |
| 500 | DB失敗 | EDGE-004 |

**信頼性メモ**: 旧PHP は `admin_delete.php?id=N` (GET) だったが、本実装は POST のみ受け付ける。

---

### 静的リソース

#### GET /uploads/* 🔵

**信頼性**: 🔵 *REQ-013 / ヒアリング「ServeDir」より*

**関連要件**: REQ-013

**説明**: アップロード済み画像の配信。`tower-http::services::ServeDir::new(upload_dir)` を `/uploads` にマウント。

**例**: `GET /uploads/5.jpg` → `uploads/5.jpg` の中身を `image/jpeg` で返す

**レスポンス**:
- 200 OK + 画像バイナリ
- 404 Not Found（ファイルが存在しない場合、ServeDir のデフォルト挙動）

**セキュリティメモ**: ServeDir は親ディレクトリへのパストラバーサル攻撃を防止する設計（NFR-103 と合わせて二重防御）。

---

## 想定外のリクエスト

### 未定義パス

axum の fallback ハンドラで 404 を返す（簡素な HTML）。

### サポート外メソッド

axum がデフォルトで 405 Method Not Allowed を返す。

---

## レート制限 🔴

**信頼性**: 🔴 *該当なし*

ローカル学習用途のため未実装。将来必要なら `tower_governor` 等を検討。

---

## バージョニング 🔴

**信頼性**: 🔴 *該当なし*

HTML エンドポイントなのでAPIバージョニングは不要。
将来 JSON API を追加する場合は `/api/v1/posts` などで切り出す。

---

## ハンドラ → 内部呼び出し対応表 🔵

**信頼性**: 🔵 *本設計の handlers と repository/storage の対応*

| エンドポイント | ハンドラ関数 | 主な呼び出し |
|---|---|---|
| `GET /` | `handlers::posts::list` | `PostRepo::list` + `count` |
| `POST /posts` | `handlers::posts::create` | `validate` → `insert` → `save_image` → `update_image_path`（失敗時 `delete`） |
| `GET /posts/:id/edit` | `handlers::posts::edit_form` | `PostRepo::find` |
| `POST /posts/:id/edit` | `handlers::posts::edit_submit` | `find` → `validate` →（必要時）`save_image`/`delete_image` → `update` |
| `POST /posts/:id/delete` | `handlers::posts::delete` | `find` → `delete_image`（任意） → `delete` |
| `GET /uploads/*` | `tower-http::ServeDir` | OS ファイルシステム |

---

## 関連文書

- **アーキテクチャ**: [architecture.md](architecture.md)
- **型定義**: [interfaces.rs](interfaces.rs)
- **データフロー**: [dataflow.md](dataflow.md)
- **DBスキーマ**: [database-schema.sql](database-schema.sql)
- **要件定義**: [../spec/requirements.md](../spec/requirements.md)

## 信頼性レベルサマリー

- 🔵 青信号: 12件 (約75%)
- 🟡 黄信号: 2件 (約13%)
- 🔴 赤信号: 2件 (約12%) — レート制限・バージョニング（該当なしの明示）

**品質評価**: 高品質
