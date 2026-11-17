//! 投稿フォームの入力（タイトル・本文・画像）を検証する。
//!
//! データの流れ:
//! 1. ハンドラが multipart を読んで、生の入力 `PostFormInput` を作る（まだ何も信用していない）
//! 2. `validate` が検証し、合格したものだけを `ValidatedPostForm` に詰め替える
//! 3. それ以降の処理（DB 保存・画像保存）は `ValidatedPostForm` だけを受け取るので、
//!    「検証し忘れた値を保存してしまう」ことが型の上で起こらない
//!
//! 学習メモ:
//! - `ValidatedPostForm` / `ValidatedImage` のフィールドは `pub` を付けていない（非公開）。
//!   Rust では非公開フィールドを持つ構造体は、定義したモジュールの外から `ValidatedPostForm { ... }` で作れない。
//!   だから「検証済みの値を作れるのはこのファイルの `validate` だけ」がコンパイラによって保証される。
//!   外からは `title()` などの読み取り専用メソッドで中身を参照する。

use crate::config::AppConfig;
use crate::errors::{AppError, AppResult};
use crate::models::image_ext::ImageExt;

/// タイトルの最大文字数（EDGE-101）。
const TITLE_MAX_CHARS: usize = 50;
/// 本文の最大文字数（EDGE-102）。
const TEXT_MAX_CHARS: usize = 500;

/// 投稿フォームの生の入力。multipart から取り出したままの、まだ検証していない値。
///
/// 検証に失敗したときは、この値をそのままテンプレートへ渡してフォームに書き戻す（入力値保持、REQ-201）。
/// `#[derive(Default)]` で、全フィールドが空（空文字・None・false）の値を `PostFormInput::default()` で作れる。
#[derive(Debug, Default, Clone)]
pub struct PostFormInput {
    /// タイトル欄の入力
    pub title: String,
    /// 本文欄の入力
    pub text: String,
    /// 画像ファイルの中身。ファイルが選ばれていなければ `None`。
    pub image_bytes: Option<Vec<u8>>,
    /// クライアントが申告した元のファイル名。画像形式の判定には使わない（偽装できるため）。
    pub image_filename: Option<String>,
    /// 「画像を削除する」チェックボックス（編集画面でのみ意味を持つ）
    pub delete_image: bool,
}

/// 検証済みの画像。中身のバイト列が jpg / gif / png のどれかであることを確認済み。
#[derive(Debug, Clone)]
pub struct ValidatedImage {
    bytes: Vec<u8>,
    extension: ImageExt,
}

impl ValidatedImage {
    /// 画像のバイト列。
    ///
    /// 学習メモ: 戻り値 `&[u8]` は「バイト列を借りて見るだけ」の参照。
    /// `Vec<u8>` を返すと所有権ごと渡す（またはコピーする）ことになるが、参照なら数 MB のデータでもコピーが起きない。
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// ファイルの中身から判定した拡張子。
    pub fn extension(&self) -> ImageExt {
        self.extension
    }
}

/// 検証済みの投稿フォーム。`validate` だけが作れる。
#[derive(Debug, Clone)]
pub struct ValidatedPostForm {
    title: String,
    text: String,
    image: Option<ValidatedImage>,
    delete_image: bool,
}

impl ValidatedPostForm {
    /// 前後の空白を取り除いた、1〜50文字のタイトル。
    pub fn title(&self) -> &str {
        &self.title
    }

    /// 0〜500文字の本文。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 検証済みの画像。画像が添付されていなければ `None`。
    ///
    /// 学習メモ: `Option<ValidatedImage>` を `.as_ref()` すると `Option<&ValidatedImage>` になる。
    /// 中身を取り出さずに「参照の Option」として貸し出せる。
    pub fn image(&self) -> Option<&ValidatedImage> {
        self.image.as_ref()
    }

    /// 編集で、投稿の画像をどうするかを決める（api-endpoints.md「画像の取り扱い」の表そのもの）。
    ///
    /// | 新しい画像 | 削除チェック | 結果      |
    /// |------------|--------------|-----------|
    /// | あり       | どちらでも   | `Replace` |
    /// | なし       | ON           | `Remove`  |
    /// | なし       | OFF          | `Keep`    |
    ///
    /// 新しい画像と削除チェックが両方あるときは、差し替えを優先する（削除チェックは無視）。
    /// 画像を選んだ利用者の意図は「この画像にしたい」なので、それを捨てて画像なしにはしない。
    ///
    /// 学習メモ:
    /// - 戻り値の `ImageChange<'_>` の `'_` は「`&self` から借りている」という印。
    ///   `Replace` が持つ画像の参照は、このフォームが生きている間だけ使える。
    /// - `match (a, b)` のようにタプルで2つの値を同時に判定できる。`_` は「何でもよい」。
    ///   上から順に試すので、1行目に当てはまれば削除チェックの値は見ない。
    pub fn image_change(&self) -> ImageChange<'_> {
        match (&self.image, self.delete_image) {
            (Some(image), _) => ImageChange::Replace(image),
            (None, true) => ImageChange::Remove,
            (None, false) => ImageChange::Keep,
        }
    }
}

/// 編集で投稿の画像をどう変えるか。`ValidatedPostForm::image_change` だけが決める。
///
/// 学習メモ:
/// - 「新しい画像があるか」「削除チェックが ON か」を使う側があちこちで組み合わせて判定すると、
///   場所ごとに解釈が食い違いやすい。組み合わせの解釈は1か所で済ませ、結果を enum で渡す。
///   使う側は `match` で全バリアントを扱わないとコンパイルできないので、扱い忘れも起きない。
/// - `<'a>` はライフタイム引数。`Replace(&'a ValidatedImage)` は検証済みフォームの画像を借りているだけで、
///   数 MB のバイト列を複製しない。借用元のフォームより長く使おうとするとコンパイルエラーになる。
pub enum ImageChange<'a> {
    /// 画像はそのまま。`image` 列もファイルも触らない。
    Keep,
    /// この画像に差し替える。古い画像ファイルは消す（拡張子が変わる場合も）。
    Replace(&'a ValidatedImage),
    /// 画像だけを削除する。`image` 列を NULL にし、ファイルも消す（REQ-103）。
    Remove,
}

/// 投稿フォームの入力を検証する。
///
/// 合格なら `Ok(ValidatedPostForm)`、不合格なら `Err(AppError::Validation(メッセージ一覧))` を返す。
/// 1つ目のエラーで止めず、見つかったエラーをすべて集めて返す（利用者が一度に直せるように）。
///
/// 学習メモ:
/// - 引数 `&PostFormInput` は借用。検証に失敗しても呼び出し元に入力値が残るので、
///   ハンドラはそれをフォームの再表示に使える。
/// - 文字数は `chars().count()` で数える。`len()` は UTF-8 の **バイト数** を返すので、
///   日本語（1文字 3 バイト）だと「あ」×17 で 51 バイトになり、誤って 50 文字超え扱いになってしまう。
pub fn validate(input: &PostFormInput, config: &AppConfig) -> AppResult<ValidatedPostForm> {
    // `mut` を付けた変数だけが後から変更（push）できる。Rust の変数は既定で不変。
    let mut errors: Vec<String> = Vec::new();

    // 前後の空白・改行だけのタイトルは空とみなす。`trim()` は元の文字列の一部を指す &str を返す（コピーしない）
    let title = input.title.trim();
    let title_len = title.chars().count();
    if title_len == 0 {
        errors.push("タイトルは必須です".to_string());
    } else if title_len > TITLE_MAX_CHARS {
        errors.push(format!(
            "タイトルは{TITLE_MAX_CHARS}文字以内で入力してください"
        ));
    }

    if input.text.chars().count() > TEXT_MAX_CHARS {
        errors.push(format!("本文は{TEXT_MAX_CHARS}文字以内で入力してください"));
    }

    // NUL 文字（U+0000）は受け付けない。
    // SQLite の length() は NUL の手前で数えるのをやめるので、"\0abc" は Rust では 4 文字でも
    // DB の CHECK 制約（1〜50文字）では 0 文字と数えられ、INSERT / UPDATE が失敗して 500 になる。
    // 「Rust 側の数え方」と「DB 側の数え方」が食い違う入力は、入口のここで弾いておく。
    if input.title.contains('\0') || input.text.contains('\0') {
        errors.push("タイトルと本文に使えない文字が含まれています".to_string());
    }

    // 画像は任意。`match` で「無い」「空」「大きすぎる」「形式を判定する」に分ける。
    // `&input.image_bytes` と参照で受けるので、中の Vec<u8> は所有権を移さずに覗くだけ。
    let image = match &input.image_bytes {
        None => None,
        // ファイルを選んだのに中身が 0 バイト（EDGE-103）
        Some(bytes) if bytes.is_empty() => {
            errors.push("画像ファイルが空です".to_string());
            None
        }
        // 上限ちょうど（5MB = 5,242,880 バイト）は受け付ける（TC-004-B05）
        Some(bytes) if bytes.len() > config.max_image_bytes => {
            errors.push("画像サイズは5MB以内にしてください".to_string());
            None
        }
        Some(bytes) => {
            // `infer::get` は先頭バイトから形式を推定し、分からなければ `None` を返す。
            // `Option` のメソッドをつなげて「判定できた」かつ「許可リストにある」かつ「ImageExt に変換できる」
            // ときだけ `Some(ImageExt)` になるようにしている。どこかで外れれば `None`。
            let extension = infer::get(bytes)
                .map(|kind| kind.mime_type())
                .filter(|mime| config.allowed_mime.contains(mime))
                .and_then(ImageExt::from_mime);
            match extension {
                Some(extension) => Some(ValidatedImage {
                    // 入力は借用しているだけなので、検証済みの値として持つために複製（clone）する
                    bytes: bytes.clone(),
                    extension,
                }),
                None => {
                    errors.push("対応していない画像形式です (jpg/gif/png)".to_string());
                    None
                }
            }
        }
    };

    if !errors.is_empty() {
        return Err(AppError::Validation(errors));
    }
    Ok(ValidatedPostForm {
        // `to_string()` で借りていた &str から、所有する String を新しく作る
        title: title.to_string(),
        text: input.text.clone(),
        image,
        delete_image: input.delete_image,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 各形式の先頭バイト（マジックナンバー）。後ろは何でもよい。
    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
    const GIF_MAGIC: &[u8] = b"GIF89a";
    const JPEG_MAGIC: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0];

    fn config() -> AppConfig {
        AppConfig::from_env_or_default()
    }

    fn input(title: &str, text: &str, image: Option<Vec<u8>>) -> PostFormInput {
        PostFormInput {
            title: title.to_string(),
            text: text.to_string(),
            image_bytes: image,
            ..PostFormInput::default()
        }
    }

    /// 指定の先頭バイトに 0 を足して、ちょうど `len` バイトの画像データを作る。
    fn image_of_len(magic: &[u8], len: usize) -> Vec<u8> {
        let mut bytes = magic.to_vec();
        bytes.resize(len, 0);
        bytes
    }

    /// 検証に失敗することを確かめ、エラーメッセージの一覧を返す。
    fn errors_of(input: &PostFormInput) -> Vec<String> {
        match validate(input, &config()) {
            Err(AppError::Validation(messages)) => messages,
            other => panic!("Validation エラーになるはず: {other:?}"),
        }
    }

    fn validated(input: &PostFormInput) -> ValidatedPostForm {
        validate(input, &config()).expect("検証に合格するはず")
    }

    #[test]
    fn title_empty_returns_error() {
        assert_eq!(errors_of(&input("", "本文", None)), ["タイトルは必須です"]);
    }

    #[test]
    fn title_with_only_spaces_returns_error() {
        assert_eq!(
            errors_of(&input(" \u{3000}\n", "", None)),
            ["タイトルは必須です"]
        );
    }

    #[test]
    fn nul_character_in_title_or_text_returns_error() {
        let message = ["タイトルと本文に使えない文字が含まれています"];
        assert_eq!(errors_of(&input("\0abc", "", None)), message);
        assert_eq!(errors_of(&input("ab\0c", "", None)), message);
        assert_eq!(errors_of(&input("タイトル", "本文\0", None)), message);
    }

    #[test]
    fn title_51_chars_returns_error() {
        assert_eq!(
            errors_of(&input(&"あ".repeat(51), "", None)),
            ["タイトルは50文字以内で入力してください"]
        );
    }

    #[test]
    fn title_50_chars_is_ok() {
        // 日本語50文字は150バイト。バイト数ではなく文字数で数えていれば合格する
        let title = "あ".repeat(50);
        assert_eq!(validated(&input(&title, "", None)).title(), title);
    }

    #[test]
    fn title_is_trimmed() {
        assert_eq!(validated(&input("  hello  ", "", None)).title(), "hello");
    }

    #[test]
    fn text_501_chars_returns_error() {
        assert_eq!(
            errors_of(&input("t", &"あ".repeat(501), None)),
            ["本文は500文字以内で入力してください"]
        );
    }

    #[test]
    fn text_500_chars_is_ok() {
        let text = "あ".repeat(500);
        assert_eq!(validated(&input("t", &text, None)).text(), text);
    }

    #[test]
    fn empty_text_without_image_is_ok() {
        let form = validated(&input("t", "", None));
        assert_eq!(form.text(), "");
        assert!(form.image().is_none());
    }

    #[test]
    fn image_over_5mb_returns_error() {
        let image = image_of_len(PNG_MAGIC, 5 * 1024 * 1024 + 1);
        assert_eq!(
            errors_of(&input("t", "", Some(image))),
            ["画像サイズは5MB以内にしてください"]
        );
    }

    #[test]
    fn image_of_exactly_5mb_is_ok() {
        let image = image_of_len(PNG_MAGIC, 5 * 1024 * 1024);
        let form = validated(&input("t", "", Some(image.clone())));
        assert_eq!(form.image().unwrap().bytes(), image);
    }

    #[test]
    fn image_invalid_mime_returns_error() {
        let image = b"this is not an image".to_vec();
        assert_eq!(
            errors_of(&input("t", "", Some(image))),
            ["対応していない画像形式です (jpg/gif/png)"]
        );
    }

    #[test]
    fn pdf_named_like_an_image_returns_error() {
        // ファイル名が .jpg でも、中身が PDF なら拒否する（TC-004-E07）
        let form = PostFormInput {
            image_filename: Some("evil.jpg".to_string()),
            ..input("t", "", Some(b"%PDF-1.4\n%...".to_vec()))
        };
        assert_eq!(
            errors_of(&form),
            ["対応していない画像形式です (jpg/gif/png)"]
        );
    }

    #[test]
    fn empty_image_returns_error() {
        assert_eq!(
            errors_of(&input("t", "", Some(Vec::new()))),
            ["画像ファイルが空です"]
        );
    }

    #[test]
    fn image_jpeg_magic_bytes_is_ok() {
        let form = validated(&input("t", "", Some(JPEG_MAGIC.to_vec())));
        assert_eq!(form.image().unwrap().extension(), ImageExt::Jpg);
    }

    #[test]
    fn extension_comes_from_content_not_file_name() {
        for (magic, expected) in [(PNG_MAGIC, ImageExt::Png), (GIF_MAGIC, ImageExt::Gif)] {
            let form = PostFormInput {
                image_filename: Some("photo.jpg".to_string()),
                ..input("t", "", Some(magic.to_vec()))
            };
            assert_eq!(validated(&form).image().unwrap().extension(), expected);
        }
    }

    #[test]
    fn all_errors_are_returned_together() {
        let bad = input("", &"x".repeat(501), Some(b"not an image".to_vec()));
        assert_eq!(
            errors_of(&bad),
            [
                "タイトルは必須です",
                "本文は500文字以内で入力してください",
                "対応していない画像形式です (jpg/gif/png)",
            ]
        );
    }

    /// 新しい画像と削除チェックの組み合わせから決まる `ImageChange` を、比べやすい名前で返す。
    fn change_of(image: Option<Vec<u8>>, delete_image: bool) -> &'static str {
        let form = validated(&PostFormInput {
            delete_image,
            ..input("t", "", image)
        });
        match form.image_change() {
            ImageChange::Keep => "keep",
            ImageChange::Replace(_) => "replace",
            ImageChange::Remove => "remove",
        }
    }

    #[test]
    fn image_change_follows_the_new_image_and_the_delete_checkbox() {
        assert_eq!(change_of(None, false), "keep");
        assert_eq!(change_of(None, true), "remove");
        assert_eq!(change_of(Some(PNG_MAGIC.to_vec()), false), "replace");
        // 両方指定されたら差し替えを優先する（api-endpoints.md）
        assert_eq!(change_of(Some(PNG_MAGIC.to_vec()), true), "replace");
    }

    #[test]
    fn image_ext_strings_are_fixed() {
        assert_eq!(
            ImageExt::from_mime("image/jpeg").map(ImageExt::as_str),
            Some("jpg")
        );
        assert_eq!(
            ImageExt::from_mime("image/gif").map(ImageExt::as_str),
            Some("gif")
        );
        assert_eq!(
            ImageExt::from_mime("image/png").map(ImageExt::as_str),
            Some("png")
        );
        assert_eq!(ImageExt::from_mime("image/webp"), None);
    }
}
