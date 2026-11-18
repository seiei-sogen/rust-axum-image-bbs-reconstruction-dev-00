//! 一覧画面のページネーション（`?page=N`）。
//!
//! - `PaginationQuery`: クエリ文字列 `?page=N` を受け取る型。
//! - `paginate`: 「要求ページ・総件数・1ページの件数」から、実際に表示するページ・総ページ数・OFFSET を求める純粋関数。
//!   ページ計算はすべてこの関数に集めてあり、DB も HTTP も使わずにユニットテストできる。
//!
//! 学習メモ:
//! - `page` を `u32`（符号なし）で受けるので、`?page=-1` や `?page=abc` は数値への変換自体が失敗する。
//!   axum の `Query<T>` は変換に失敗すると 400 を返してしまうため、ハンドラ側では
//!   `Option<Query<PaginationQuery>>` として受け取り、失敗（`None`）なら 1 ページ目として扱う。

use serde::Deserialize;

/// `?page=N` を受け取るクエリ型。
///
/// `#[derive(Deserialize)]` により、axum の `Query` がクエリ文字列からこの構造体を組み立てられる。
#[derive(Debug, Clone, Deserialize)]
pub struct PaginationQuery {
    /// 1始まりのページ番号。`?page` が付いていなければ `default_page()` の 1 になる。
    #[serde(default = "default_page")]
    pub page: u32,
}

/// `#[serde(default = "...")]` には「値を返す関数の名前」を文字列で渡す決まり。
fn default_page() -> u32 {
    1
}

/// `paginate` の計算結果。
///
/// `Copy` を派生しているので、代入や関数呼び出しで所有権がムーブされず、ビット単位で複製される
/// （u32 だけを持つ小さな構造体なので複製コストはほぼゼロ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    /// 実際に表示するページ（1〜total_pages に収めたもの）。
    pub page: u32,
    /// 総ページ数。投稿0件でも 1。
    pub total_pages: u32,
    /// SQL の `OFFSET` に渡す、先頭から読み飛ばす件数。
    pub offset: u32,
}

/// 要求されたページ番号を有効範囲に収め、総ページ数と OFFSET を計算する。
///
/// - `requested_page`: ユーザーが `?page=N` で指定した値（0 や範囲外もありうる）
/// - `total`: 投稿の総件数
/// - `page_size`: 1ページの件数（1 以上。AppConfig の既定は 20）
pub fn paginate(requested_page: u32, total: u32, page_size: u32) -> Pagination {
    // 割り算の切り上げ（45件 / 20件 = 3ページ）。整数だけで計算するので浮動小数の誤差がない。
    // 投稿0件だと 0 ページになってしまうので、最低 1 ページにする。
    let total_pages = total.div_ceil(page_size).max(1);
    // 0 → 1、最終ページより後 → 最終ページ、に丸める。total_pages >= 1 なので clamp の範囲は常に正しい
    let page = requested_page.clamp(1, total_pages);
    // page は 1 以上なので `page - 1` が負（u32 のアンダーフローでパニック）になることはない。
    // また page <= total_pages なので、offset が総件数を超えて u32 からあふれることもない。
    let offset = (page - 1) * page_size;

    Pagination {
        page,
        total_pages,
        offset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1ページ20件・総件数45件（= 20 + 20 + 5 の3ページ）を基準にする
    const PAGE_SIZE: u32 = 20;

    #[test]
    fn first_page_starts_at_offset_zero() {
        assert_eq!(
            paginate(1, 45, PAGE_SIZE),
            Pagination {
                page: 1,
                total_pages: 3,
                offset: 0
            }
        );
    }

    #[test]
    fn second_page_skips_one_page_of_posts() {
        assert_eq!(paginate(2, 45, PAGE_SIZE).offset, 20);
    }

    #[test]
    fn last_page_skips_all_full_pages() {
        assert_eq!(
            paginate(3, 45, PAGE_SIZE),
            Pagination {
                page: 3,
                total_pages: 3,
                offset: 40
            }
        );
    }

    #[test]
    fn total_pages_rounds_up_partial_pages() {
        assert_eq!(paginate(1, 40, PAGE_SIZE).total_pages, 2);
        assert_eq!(paginate(1, 41, PAGE_SIZE).total_pages, 3);
        assert_eq!(paginate(1, 1, PAGE_SIZE).total_pages, 1);
    }

    #[test]
    fn page_zero_falls_back_to_first_page() {
        assert_eq!(paginate(0, 45, PAGE_SIZE), paginate(1, 45, PAGE_SIZE));
    }

    #[test]
    fn page_beyond_last_clamps_to_last_page() {
        assert_eq!(paginate(4, 45, PAGE_SIZE), paginate(3, 45, PAGE_SIZE));
        assert_eq!(
            paginate(u32::MAX, 45, PAGE_SIZE),
            paginate(3, 45, PAGE_SIZE)
        );
    }

    #[test]
    fn no_posts_still_has_one_page() {
        let expected = Pagination {
            page: 1,
            total_pages: 1,
            offset: 0,
        };
        assert_eq!(paginate(1, 0, PAGE_SIZE), expected);
        assert_eq!(paginate(5, 0, PAGE_SIZE), expected);
    }
}
