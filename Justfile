# rust-axum-image-bbs 用のタスクランナー設定
#
# 使い方:
#   just                 # 一覧を表示
#   just run             # axum サーバを起動
#   just verify          # fmt + lint + test を一括実行
#
# 注意:
#   - Justfile のレシピはタスクが増えるたびに追記していく予定
#   - cargo 周りは小さなラッパだが、コマンドを覚えなくてよくなる効果が大きい

# 既定: レシピ一覧を表示する
default:
    @just --list

# axum サーバを起動する（cargo run 相当）
run:
    cargo run

# リリースビルドした成果物で起動する
run-release: release
    ./target/release/rust-axum-image-bbs

# デバッグビルド
build:
    cargo build

# リリースビルド（最適化あり）
release:
    cargo build --release

# 高速なコンパイルチェック（コード生成なし）
check:
    cargo check

# 単体テスト・統合テストを実行
test:
    cargo test

# コードフォーマット
fmt:
    cargo fmt

# フォーマットされているかの確認のみ（CI 用）
fmt-check:
    cargo fmt -- --check

# Lint。警告はエラー扱いにして見逃さないようにする
# --all-targets で、本体だけでなく tests/ の統合テストやユニットテストも検査する
lint:
    cargo clippy --all-targets -- -D warnings

# 動作確認: サーバが起動している前提で `/` を curl で叩く
smoke:
    curl -i http://localhost:3000/

# 実サーバを一時 DB で起動し、curl で投稿作成〜削除まで通しで確認する
# 例: `just e2e` / `just e2e create`（編集・削除を実装する前の段階用）
e2e upto="all":
    ./scripts/e2e.sh {{upto}}

# 依存クレートを最新の互換バージョンに更新
update:
    cargo update

# ビルド成果物を削除する（target/ を消す）
clean:
    cargo clean

# 一括チェック: フォーマット → Lint → テスト
verify: fmt-check lint test
    @echo "✅ verify が通った"

# 開発用ホットリロード（事前に `cargo install cargo-watch` が必要）
watch:
    cargo watch -x run
