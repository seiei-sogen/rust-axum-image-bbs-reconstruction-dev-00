# TASK-0018 受け入れテストの対応表

[acceptance-criteria.md](../../spec/acceptance-criteria.md) の全テストケースと、それを確かめている自動テストの対応。
テストはすべて `cargo test` で動く。`ファイル::関数` は `tests/` 配下の統合テスト（`build_app` のルーターに HTTP リクエストを投げる）。

- 全37件を自動テストで確認している（自動化できないものは無し）。
  acceptance-criteria.md のサマリー表は合計36件だが、非機能要件を4件と数えている。本文には5件（TC-NFR-001-01 / 101 / 102 / 103 / 104）ある
- エラーメッセージの文面は [TASK-0009](../../tasks/TASK-0009.md) で決めたもの（例:「タイトルは50文字以内で入力してください」）。
  acceptance-criteria.md に書かれた旧 PHP の文面（「タイトルは50文字までにしてください。」）とは違う
- 画像の保存先は `img/uploads/` ではなく `UPLOAD_DIR`（既定 `uploads/`）、URL は `/uploads/{id}.{ext}`（[api-endpoints.md](../../design/api-endpoints.md) に従う）

## 一覧（REQ-001 / 002 / 012）

| TC | 確かめる内容 | テスト |
|---|---|---|
| TC-001-01 | 0件で 200、空状態メッセージ | `integration_list::list_shows_empty_message_when_no_posts` |
| TC-001-02 | 画像つき1件のカードに画像・タイトル・本文・日付 | `integration_list::single_post_with_image_shows_image_title_text_and_date_in_one_card` |
| TC-001-03 | 25件で1ページ目に新しい順の20件、残り5件は2ページ目 | `integration_list::pagination_shows_20_posts_then_the_remaining_5` |
| TC-001-04 | 画像なしの投稿には `<img>` が出ない | `integration_list::list_shows_image_only_for_posts_with_image` |
| TC-001-E01 | `?page=-1` / `?page=abc` は 200 で1ページ目 | `integration_list::invalid_page_falls_back_to_first_page` |

## 作成（REQ-003〜006）

| TC | 確かめる内容 | テスト |
|---|---|---|
| TC-004-01 | title + text + jpg で行ができ、`{id}.jpg` に保存、`image` 列に記録 | `integration_create::jpeg_with_title_and_text_is_saved_as_id_jpg` |
| TC-004-02 | 画像なしで行ができ、`image` は NULL、ファイルは作られない | `integration_create::create_inserts_post_and_redirects_with_303` |
| TC-004-03 | png は `{id}.png` で保存 | `integration_create::png_is_saved_as_id_png_and_served_from_uploads` |
| TC-004-04 | gif は `{id}.gif` で保存 | `integration_create::gif_is_saved_as_id_gif` |
| TC-004-E01 | title 空で 400、メッセージ、本文は残る、行は増えない | `integration_create::empty_title_rerenders_form_with_input_kept` |
| TC-004-E02 | title 51文字で 400、メッセージ、行は作られない | `integration_create::title_of_51_chars_is_rejected_and_text_is_kept` |
| TC-004-E03 | text 501文字で 400、メッセージ、行は作られない | `integration_create::text_of_501_chars_is_rejected_and_title_is_kept` |
| TC-004-E04 | pdf は 400、形式のメッセージ、行もファイルも作られない | `integration_create::other_formats_are_rejected_even_when_named_like_images` |
| TC-004-E05 | 5MB 超は拒否、サイズのメッセージ、行もファイルも作られない | `integration_create::image_just_over_5mb_is_a_validation_error`（5MB + 1 バイトは 400）と `integration_create::body_far_over_the_limit_returns_413`（6MB はボディ上限で 413） |
| TC-004-E06 | 0バイトの画像は 400 | `integration_create::zero_byte_image_file_is_rejected` |
| TC-004-E07 | 中身が pdf で名前が `.jpg` でも 400 | `integration_create::other_formats_are_rejected_even_when_named_like_images` |
| TC-004-B01 | title 1文字は受理 | `integration_create::shortest_and_longest_allowed_lengths_are_saved` |
| TC-004-B02 | title 50文字は受理 | `integration_create::japanese_title_of_50_chars_is_accepted` |
| TC-004-B03 | text 0文字は受理 | `integration_create::shortest_and_longest_allowed_lengths_are_saved` |
| TC-004-B04 | text 500文字は受理 | `integration_create::shortest_and_longest_allowed_lengths_are_saved` |
| TC-004-B05 | ちょうど 5MB（5,242,880 バイト）は受理して保存 | `integration_create::image_of_exactly_5mb_is_saved` |

TC-004-E05 の 400 と 413 の分かれ目: 画像の上限（5MB）にボディの余裕（64KiB）を足した値をボディ上限にしている（`src/lib.rs` の `MULTIPART_OVERHEAD_BYTES`）。
それ以下ならバリデータがフォームを再表示し（400）、それより大きければボディを読み切る前に打ち切る（413、[api-endpoints.md](../../design/api-endpoints.md) の表どおり）。

## 編集（REQ-008 / 009 / 104）

| TC | 確かめる内容 | テスト |
|---|---|---|
| TC-009-01 | title だけ変えると、本文と画像はそのまま、`update_date` は更新 | `integration_edit::edit_of_title_only_keeps_text_and_image_and_refreshes_update_date` |
| TC-009-02 | 新しい画像で差し替え、拡張子が変われば古いファイルは消える | `integration_edit::edit_with_new_image_replaces_it_and_removes_the_old_extension` / `integration_edit::edit_with_new_image_of_the_same_extension_overwrites_it` |
| TC-009-03 | `delete_image=on` で `image` は NULL、ファイルも消える | `integration_edit::delete_image_checkbox_removes_only_the_image` |
| TC-009-E01 | `GET /posts/9999/edit` は 404 | `integration_edit::edit_form_for_missing_id_returns_404_page`（POST も `integration_edit::edit_of_missing_post_returns_404_page`） |
| TC-009-E02 | title 51文字で 400、編集画面を再表示、入力値は残り、行は変わらない | `integration_edit::edit_with_51_char_title_is_rejected_and_keeps_the_input` |

## 削除（REQ-010 / 011 / 102）

| TC | 確かめる内容 | テスト |
|---|---|---|
| TC-010-01 | 画像つきの投稿を消すと行もファイルも消え、303 で一覧へ | `integration_delete::delete_removes_the_post_and_its_image` |
| TC-010-02 | 画像なしの投稿もエラーなく消せる | `integration_delete::delete_of_post_without_image_succeeds` |
| TC-010-E01 | 存在しない id は 404 | `integration_delete::delete_of_missing_post_returns_404_page` |
| TC-010-E02 | `GET /posts/{id}/delete` は 405 で、何も消えない | `integration_delete::get_delete_returns_405_and_deletes_nothing` |

## 非機能要件

| TC | 確かめる内容 | テスト |
|---|---|---|
| TC-NFR-001-01 | 100件で一覧の応答が 500ms 以内 | `integration_list::list_of_100_posts_renders_within_500ms` |
| TC-NFR-101-01 | title の `'); DROP TABLE post; --` はそのまま保存され、テーブルは壊れない | `integration_create::sql_in_the_title_is_saved_as_plain_text` |
| TC-NFR-102-01 | 実行ファイルを `.jpg` に改名しても 400 | `integration_create::other_formats_are_rejected_even_when_named_like_images` |
| TC-NFR-103-01 | ファイル名 `../../...` を送っても `{id}.{ext}` で upload_dir の中に保存 | `integration_create::saved_extension_comes_from_content_not_file_name` |
| TC-NFR-104-01 | title の `<script>` はエスケープして表示 | `integration_list::list_escapes_html_in_posts` |

TC-NFR-001-01 のテストが測るのはルーターの処理（DB の問い合わせと HTML の描画）だけで、ネットワークは含まない。
実サーバ（デバッグビルド、ファイルの SQLite、100件）に curl で `GET /` を5回送ったときの TTFB は 1.5〜2.7ms だった（2026-11-17 計測）。
一覧の処理は COUNT と 20件の SELECT と描画だけなので、目標の 500ms よりずっと小さいのは想定どおり。

## Edge ケース

| TC | 確かめる内容 | テスト |
|---|---|---|
| TC-EDGE-004-01 | DB が使えないと 500 で、原因を ERROR ログに出す | `integration_errors::unmigrated_db_returns_500_and_logs_the_cause_as_error`（マイグレーション未実行の DB）、`integration_errors::db_failure_returns_500_page_without_internal_details`（接続プールが閉じた DB） |
| TC-EDGE-005-01 | 画像を保存できないと、作った行を消して最終状態をそろえる | `integration_create::failed_image_save_rolls_back_the_post` |

TC-EDGE-004-01 の「DB ファイル不在で起動」は、そのままの形では起きない。起動処理（`src/main.rs`）がファイルとその親ディレクトリを作り、マイグレーションも当てるから。
そこでテストでは、起動処理を通らずにアプリを組み立てて「テーブルの無い DB」を作り、その状態で 500 とログを確かめる。

TC-EDGE-005-01 の「読み取り専用のディレクトリ」は、テストを root で動かすと書き込めてしまうので、保存先のファイル名（`1.png`）にディレクトリを置いて書き込みを失敗させている。
