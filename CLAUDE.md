# rust-axum-image-bbs

旧PHP画像掲示板を Rust + axum で再実装する学習用プロジェクト。

## プロジェクト方針

- **Rust 初心者向けの学習プロジェクト**。実装過程は Zenn 記事として段階的に公開する想定。
- **ソースコード内のコメントは日本語で詳細に**。所有権 / 借用 / `Result<T, E>` / `?` / `async/.await` など Rust 特有の概念は初学者向けに補足する。
- 技術選定: `axum 0.7` / `sqlx 0.8` (SQLite) / `Askama 0.12` / `tracing` / `tower-http`。詳細は `docs/tumigidoc/rust-axum-image-bbs/spec/note.md` を参照。

## 関連ドキュメント

- 要件・設計: `docs/tumigidoc/rust-axum-image-bbs/`
- タスク一覧: `docs/tumigidoc/rust-axum-image-bbs/tasks/overview.md`

## 開発コマンド

### サーバー起動

```bash
# 開発サーバー起動（デフォルト info レベルログ）
cargo run

# debug レベルログで起動（より詳細なログが出る）
RUST_LOG=debug cargo run
```

### ビルド

```bash
# リリースビルド
cargo build --release
```

### テスト

```bash
# テスト実行（現状テストは無いが今後のため）
cargo test
```

### 動作確認

```bash
# ルートエンドポイントへのリクエスト確認
curl http://localhost:3000/
```
