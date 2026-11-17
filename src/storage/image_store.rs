//! アップロード画像をディスクに保存・削除する。
//!
//! 保存先は `{upload_dir}/{投稿id}.{拡張子}`。ファイル名は投稿 id と `ImageExt::as_str()` だけから作り、
//! クライアントが送ってきたファイル名は一切使わない。
//! だから `../../etc/passwd` のような名前を送られても、upload_dir の外には書けない（NFR-103）。
//!
//! 学習メモ:
//! - `std::fs` ではなく `tokio::fs` を使う。`std::fs::write` は書き終わるまでスレッドを止めてしまい、
//!   その間 tokio は同じスレッドで他のリクエストを進められない。
//!   `tokio::fs::write(...).await` なら、待っている間に他のリクエストを処理できる。
//! - `?` を付けると `std::io::Error` は `From` 実装で `AppError::Io` に変換されて返る。

use std::io::ErrorKind;
use std::path::Path;

use crate::errors::AppResult;
use crate::models::image_ext::ImageExt;
use crate::validators::post_form::ValidatedImage;

/// 画像を `{upload_dir}/{id}.{拡張子}` に保存し、DB の `image` 列に入れるファイル名（例: `"5.png"`）を返す。
///
/// 学習メモ:
/// - `upload_dir: &Path` は `PathBuf` の借用版（`String` に対する `&str` と同じ関係）。
///   呼び出し側は `&state.upload_dir` と書くだけで `&PathBuf` が `&Path` に自動変換される。
pub async fn save_image(upload_dir: &Path, id: i64, image: &ValidatedImage) -> AppResult<String> {
    // 起動時にも作っているが、実行中に消された場合に備えて毎回作る（既にあれば何もしない）
    tokio::fs::create_dir_all(upload_dir).await?;
    let filename = file_name(id, image.extension());
    // `join` はパスを OS の区切り文字でつなげる（`uploads` + `5.png` → `uploads/5.png`）
    tokio::fs::write(upload_dir.join(&filename), image.bytes()).await?;
    Ok(filename)
}

/// 投稿 id の画像を、拡張子を問わず削除する。ファイルが無くてもエラーにしない。
///
/// 編集で jpg → png に差し替えたとき、古い `5.jpg` を確実に消すために3種類とも試す。
pub async fn delete_image(upload_dir: &Path, id: i64) -> AppResult<()> {
    for ext in [ImageExt::Jpg, ImageExt::Gif, ImageExt::Png] {
        match tokio::fs::remove_file(upload_dir.join(file_name(id, ext))).await {
            Ok(()) => {}
            // 「そもそも無い」は消えているのと同じなので成功扱い（何度呼んでも同じ結果になる）
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            // `err.into()` は `From` 実装を使って `std::io::Error` → `AppError::Io` に変換する
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

/// 保存ファイル名 `{id}.{拡張子}`。ファイル名を作るのはこの関数だけ。
fn file_name(id: i64, ext: ImageExt) -> String {
    format!("{id}.{}", ext.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::validators::post_form::{validate, PostFormInput};
    use tempfile::TempDir;

    /// 検証済みの PNG 画像を作る。`ValidatedImage` はバリデータしか作れないので、本物と同じく validate を通す。
    fn png_image() -> ValidatedImage {
        let input = PostFormInput {
            title: "t".to_string(),
            image_bytes: Some(b"\x89PNG\r\n\x1a\n-png-body".to_vec()),
            ..PostFormInput::default()
        };
        let form = validate(&input, &AppConfig::from_env_or_default()).unwrap();
        form.image().unwrap().clone()
    }

    #[tokio::test]
    async fn save_image_writes_file() {
        let dir = TempDir::new().unwrap();
        let image = png_image();

        let filename = save_image(dir.path(), 1, &image).await.unwrap();

        assert_eq!(filename, "1.png");
        assert_eq!(
            std::fs::read(dir.path().join("1.png")).unwrap(),
            image.bytes()
        );
    }

    #[tokio::test]
    async fn save_image_creates_parent_dir() {
        let dir = TempDir::new().unwrap();
        let upload_dir = dir.path().join("nested").join("uploads");

        save_image(&upload_dir, 7, &png_image()).await.unwrap();

        assert!(upload_dir.join("7.png").is_file());
    }

    #[tokio::test]
    async fn delete_image_removes_existing() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("1.jpg"), b"old").unwrap();
        std::fs::write(dir.path().join("1.png"), b"new").unwrap();
        std::fs::write(dir.path().join("11.jpg"), b"other post").unwrap();

        delete_image(dir.path(), 1).await.unwrap();

        assert!(!dir.path().join("1.jpg").exists());
        assert!(!dir.path().join("1.png").exists());
        // 別の投稿（id=11）の画像は残る
        assert!(dir.path().join("11.jpg").exists());
    }

    #[tokio::test]
    async fn delete_image_ignores_missing() {
        let dir = TempDir::new().unwrap();

        delete_image(dir.path(), 999).await.unwrap();
    }
}
