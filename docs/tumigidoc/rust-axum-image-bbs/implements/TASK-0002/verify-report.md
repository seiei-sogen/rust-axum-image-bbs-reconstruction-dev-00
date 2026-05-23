# TASK-0002 検証レポート

- **タスク**: TASK-0002 — tracing + tower-http TraceLayer 導入
- **検証日**: 2026-05-23
- **担当フェーズ**: direct-verify
- **結果**: ✅ 全完了条件クリア

---

## 静的検証

### Cargo.toml 確認

| 依存クレート | バージョン | features | 状態 |
|---|---|---|---|
| `tracing` | `"0.1"` | — | ✅ |
| `tracing-subscriber` | `"0.3"` | `["env-filter"]` | ✅ |
| `tower-http` | `"0.5"` | `["trace"]` | ✅ |

### src/main.rs 確認

| チェック項目 | 状態 |
|---|---|
| `tracing_subscriber::fmt().with_env_filter(...).init()` の初期化 | ✅ |
| デフォルトフィルタ `"info,tower_http=debug"` 設定 | ✅ |
| `Router` に `.layer(TraceLayer::new_for_http())` 付与 | ✅ |
| `println!` → `tracing::info!` 置換 | ✅ |
| 日本語学習コメントの維持 | ✅ |

### cargo build

```
$ cargo build
   Compiling rust-axum-image-bbs v0.1.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.97s
```

結果: ✅ コンパイル成功（警告なし）

---

## 動作確認

### 1. 通常起動（デフォルト info レベル）

```
$ cargo run
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.02s
     Running `target/debug/rust-axum-image-bbs`
2026-05-23T04:59:52.826059Z  INFO rust_axum_image_bbs: server listening on http://0.0.0.0:3000
```

起動ログが `tracing::info!` 経由で出力されていることを確認 ✅

### 2. curl によるリクエスト確認

```
$ curl -s http://localhost:3000/
Hello, axum!
```

レスポンス `Hello, axum!` を確認 ✅

サーバー側ログ（抜粋）:

```
2026-05-23T05:00:18.632244Z DEBUG request{method=GET uri=/ version=HTTP/1.1}: tower_http::trace::on_request: started processing request
2026-05-23T05:00:18.632297Z DEBUG request{method=GET uri=/ version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=0 ms status=200
```

- `started processing request` 出力 ✅
- `finished processing request status=200` 出力 ✅

### 3. RUST_LOG=debug で起動

```
$ RUST_LOG=debug cargo run
2026-05-23T05:00:37.336931Z  INFO rust_axum_image_bbs: server listening on http://0.0.0.0:3000
```

curl 後のログ（抜粋）:

```
2026-05-23T05:00:43.713798Z DEBUG request{method=GET uri=/ version=HTTP/1.1}: tower_http::trace::on_request: started processing request
2026-05-23T05:00:43.713853Z DEBUG request{method=GET uri=/ version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=0 ms status=200
```

`RUST_LOG=debug` でも正しく DEBUG ログが出力されることを確認 ✅

---

## 完了条件チェックリスト

- [x] `Cargo.toml` に tracing/tracing-subscriber/tower-http が追加されている
- [x] `tracing_subscriber::fmt().with_env_filter(...).init()` で初期化されている
- [x] Router に `.layer(TraceLayer::new_for_http())` が付与されている
- [x] `curl http://localhost:3000/` 時にコンソールへ `started processing request` / `finished processing request status=200` 相当のログが出る
- [x] `RUST_LOG=debug cargo run` でレベルが切り替えられる
- [x] デフォルト（環境変数未設定）でも `info` レベルが出る

---

## 修正内容

静的検証・動作確認ともに問題なし。コードの修正は不要だった。

## 作成・更新ファイル

- `CLAUDE.md` — 新規作成（開発コマンドセクション追加）
- `docs/tumigidoc/rust-axum-image-bbs/implements/TASK-0002/verify-report.md` — 本ファイル（新規作成）
- `docs/tumigidoc/rust-axum-image-bbs/tasks/TASK-0002.md` — 完了マーキング更新
- `docs/tumigidoc/rust-axum-image-bbs/tasks/overview.md` — TASK-0002 完了マーキング更新
