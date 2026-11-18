# syntax=docker/dockerfile:1
#
# rust-axum-image-bbs のコンテナイメージ（マルチステージビルド）。
#
# - builder ステージ: Rust のツールチェーン入りのイメージでリリースビルドする（1GB 超の大きなイメージ）
# - 実行ステージ:     Debian の最小イメージに、できあがったバイナリ1つだけをコピーする
#
# 実行に必要なのはバイナリだけ。templates/ と migrations/ はコンパイル時にバイナリへ埋め込まれる
# （Askama の `#[template(path = ...)]` と `sqlx::migrate!`）。SQLite も sqlx が静的にリンクする。
# sqlx のクエリは実行時に組み立てる書き方（`query!` マクロを使わない）なので、ビルドに DB は要らない。

# builder と実行ステージは同じ Debian（bookworm）にそろえる。
# バイナリは glibc に動的リンクされるので、ビルドした環境より古い glibc では動かない。
FROM rust:1.97-slim-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY templates ./templates
# --mount=type=cache で、ダウンロードしたクレートとビルド途中の成果物をビルドをまたいで再利用する。
# キャッシュはイメージには入らないので、バイナリは同じ RUN の中で外へコピーしておく。
# --locked は Cargo.lock のバージョンをそのまま使う（ビルドのたびに依存が変わらないように）。
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked \
    && cp target/release/rust-axum-image-bbs /usr/local/bin/

FROM debian:bookworm-slim
# root ではなく専用ユーザ（uid 1000）で動かす。アプリに乗っ取られても、コンテナ内の root 権限は渡らない。
# 書き込むのは data/（SQLite）と uploads/（画像）だけなので、その2つだけをこのユーザの持ち物にする。
RUN useradd --uid 1000 --user-group --no-create-home app \
    && mkdir -p /app/data /app/uploads \
    && chown app:app /app/data /app/uploads
WORKDIR /app
COPY --from=builder /usr/local/bin/rust-axum-image-bbs /usr/local/bin/rust-axum-image-bbs
USER app
# 設定はすべて環境変数で渡す（`docker run -e` や docker-compose.yml の environment で上書きできる）
ENV DATABASE_URL=sqlite:///app/data/app.db \
    UPLOAD_DIR=/app/uploads \
    PORT=3000 \
    RUST_LOG=info
# DB と画像はコンテナの外（ボリューム）に置く。コンテナを作り直しても投稿が消えない。
VOLUME ["/app/data", "/app/uploads"]
EXPOSE 3000
CMD ["rust-axum-image-bbs"]
