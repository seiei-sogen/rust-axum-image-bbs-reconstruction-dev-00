//! 受け付ける画像形式（jpg / gif / png）を表す列挙型。
//!
//! 保存ファイル名は `{投稿id}.{拡張子}` で作る。拡張子をクライアントのファイル名から取らず、
//! この enum の `as_str()` が返す固定文字列だけを使うことで、
//! `../../etc/passwd` のような名前を送られても保存先がずれない（NFR-103 パストラバーサル対策）。
//!
//! 学習メモ:
//! - Rust の `enum` は「取りうる値の一覧」を型で表す。`ImageExt` 型の値は Jpg / Gif / Png の
//!   3つのどれかにしかなれないので、「bmp の画像」という状態はそもそも作れない。
//! - `#[derive(Clone, Copy)]` を付けると、代入や引数渡しでムーブされずにコピーされる。
//!   中身を持たない小さな enum なので、コピーのコストはほぼゼロ。

/// 許可された画像の拡張子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageExt {
    /// image/jpeg
    Jpg,
    /// image/gif
    Gif,
    /// image/png
    Png,
}

impl ImageExt {
    /// ファイル名に使う拡張子の文字列を返す。
    ///
    /// 学習メモ:
    /// - 引数が `&self` ではなく `self` なのは、`Copy` な型なので値で受け取っても元の値が使えなくならないから。
    /// - 戻り値の `&'static str` は「プログラム終了まで有効な文字列への参照」。
    ///   文字列リテラル `"jpg"` はバイナリに埋め込まれているので、いつ参照しても安全。
    pub fn as_str(self) -> &'static str {
        // `match` は全バリアントを書かないとコンパイルエラーになる（網羅性チェック）。
        // 将来 Webp を足したとき、ここを直し忘れることがない。
        match self {
            ImageExt::Jpg => "jpg",
            ImageExt::Gif => "gif",
            ImageExt::Png => "png",
        }
    }

    /// `infer` クレートが返す MIME タイプ文字列から変換する。許可外の形式なら `None`。
    pub fn from_mime(mime: &str) -> Option<Self> {
        match mime {
            "image/jpeg" => Some(ImageExt::Jpg),
            "image/gif" => Some(ImageExt::Gif),
            "image/png" => Some(ImageExt::Png),
            // `_` は「それ以外すべて」にマッチするワイルドカード
            _ => None,
        }
    }
}
