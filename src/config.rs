//! アプリ全体の設定値。
//!
//! ページングと画像アップロードの上限など、複数のモジュールから参照する値を1か所に集約する。
//! ここに置いておくと、ハンドラ・バリデータ・テストが同じ値を見ることが保証される。
//!
//! 学習メモ:
//! - 接続先やポートのように「環境ごとに変えたい値」（DATABASE_URL / PORT / UPLOAD_DIR）は
//!   `src/main.rs` で環境変数から読んでいる。
//!   このファイルの値は、学習用途なのでコード内の定数で固定している。
//!   環境変数で変えたくなったら `from_env_or_default` の中で `std::env::var` を読めばよい。

/// 一覧画面1ページあたりの件数。
const DEFAULT_PAGE_SIZE: u32 = 20;
/// 画像アップロードの上限バイト数（5MB）。
/// `5 * 1024 * 1024` のような定数同士の計算は、コンパイル時に済まされる。
const DEFAULT_MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// 受け付ける画像の MIME タイプ。
///
/// 学習メモ:
/// - `&[&str]` は「文字列スライスの配列への参照（スライス）」。
///   `const` に置いた値はプログラムの実行中ずっと存在するので、
///   ライフタイムは `'static`（プログラム終了まで有効）になる。
const DEFAULT_ALLOWED_MIME: &[&str] = &["image/jpeg", "image/gif", "image/png"];

/// アプリの設定値をまとめた構造体。
///
/// 学習メモ:
/// - `#[derive(Debug, Clone)]` は、`{:?}` での表示（Debug）と `.clone()` による複製（Clone）を
///   自動実装する指定。`AppState` が `Clone` を要求するので、その中身であるこの型にも必要になる。
/// - `allowed_mime` を `Vec<String>` ではなく `&'static [&'static str]` にしているのは、
///   値が定数で変わらないから。ヒープ確保が要らず、Clone しても参照（ポインタと長さ）を
///   コピーするだけで済む。
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// 一覧画面1ページあたりの件数（既定 20）
    pub page_size: u32,
    /// 画像アップロードのバイト数上限（既定 5MB）
    pub max_image_bytes: usize,
    /// 許可される MIME タイプ
    pub allowed_mime: &'static [&'static str],
}

impl AppConfig {
    /// 既定値で設定を作る。
    ///
    /// 学習メモ:
    /// - `Self` は「いま impl している型」（ここでは `AppConfig`）の別名。
    /// - 引数に `self` を取らない関数は「関連関数」と呼び、`AppConfig::from_env_or_default()` の形で呼ぶ。
    ///   他言語のコンストラクタや static メソッドに近い。
    pub fn from_env_or_default() -> Self {
        Self {
            page_size: DEFAULT_PAGE_SIZE,
            max_image_bytes: DEFAULT_MAX_IMAGE_BYTES,
            allowed_mime: DEFAULT_ALLOWED_MIME,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[test]
    fn default_config_matches_task_limits() {
        let config = AppConfig::from_env_or_default();

        assert_eq!(config.page_size, 20);
        assert_eq!(config.max_image_bytes, 5 * 1024 * 1024);
        assert_eq!(
            config.allowed_mime,
            ["image/jpeg", "image/gif", "image/png"]
        );
    }
}
