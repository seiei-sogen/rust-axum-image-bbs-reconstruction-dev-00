//! rust-axum-image-bbs のライブラリクレート。
//!
//! 学習メモ:
//! - Cargo は `src/lib.rs` があると「ライブラリクレート」、`src/main.rs` があると
//!   「バイナリクレート」として扱う。両方置くと、同じパッケージに2つのクレートが同居する。
//! - モジュールをライブラリ側で `pub mod` として公開しておくと、
//!   バイナリ（main.rs）や `tests/` 配下の統合テストから `rust_axum_image_bbs::...` で参照できる。
//! - バイナリ側だけで `mod xxx;` と宣言した場合、まだ main から呼ばれていない関数は
//!   「未使用（dead_code）」警告になる。ライブラリで `pub` にした関数は外部から使われうるので警告にならない。

pub mod errors;
pub mod models;
pub mod repository;
