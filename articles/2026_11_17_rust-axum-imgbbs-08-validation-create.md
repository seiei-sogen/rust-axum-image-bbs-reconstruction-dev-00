---
title: "Rust + axum で画像掲示板を作る (8) バリデーションと multipart での投稿作成"
emoji: "📮"
type: "tech"
topics: ["rust", "axum", "multipart", "askama", "web"]
published: false
---

## はじめに

[前回](./2026_11_17_rust-axum-imgbbs-07-pagination.md) は一覧にページ送りを付けた。投稿はまだ Python で DB に直接入れていた。今回は **ブラウザのフォームから投稿を作れる** ようにする。

- 生の入力と検証済みの入力を別の型にし、文字数と画像を検査する `validate` を書く
- `POST /posts` で `multipart/form-data` を読み、検証して INSERT する
- 検証に落ちたら入力値を残して 400 で描き直し、成功したら 303 で一覧へ戻す
- ボディの上限を「画像の上限 + 64KiB」にする

コミットは `TASK-0009: 投稿フォームのバリデータと ImageExt 型を追加`（`1c59298`）と `TASK-0010: POST /posts で投稿を作成する（画像保存はまだ）`（`7e5d6a6`）の2つだ。コードは `7e5d6a6` の時点のものを載せる。

:::message
`7e5d6a6` では、正しい画像を送ると検証は通るが、**画像はどこにも保存されない**。DB の `image` 列も NULL のままだ。`{id}.{拡張子}` での保存、`/uploads` での配信、保存失敗時の取り消しは次回の TASK-0011 で入れる。
:::

## 今回決める振る舞い

| 送信内容 | ステータス | 画面 |
|---|---|---|
| 正しいタイトルと本文（画像なし、または正しい画像） | 303 | `/?notice=created` で「投稿しました」 |
| タイトルが空か51文字以上、本文が501文字以上 | 400 | 入力値を残したフォームとエラー一覧 |
| 画像が jpg / gif / png 以外、0バイト、5MB 超 | 400 | 同上 |
| ボディ全体が「5MB + 64KiB」超 | 413 | 文字だけの応答 |
| multipart として読めない | 400 | 文字だけの応答 |

## 入力を2つの型に分ける

バリデータは `src/validators/post_form.rs` に置く。ハンドラが multipart から作る生の入力は「まだ何も信用していない」値で、`validate` に合格したものだけを検証済みの型に詰め替える。

```rust:src/validators/post_form.rs
#[derive(Debug, Default, Clone)]
pub struct PostFormInput {
    pub title: String,
    pub text: String,
    pub image_bytes: Option<Vec<u8>>,
    pub image_filename: Option<String>,
    pub delete_image: bool,
}
```

生の入力はフィールドがすべて `pub` で、ハンドラが届いた欄を1つずつ書き込む。`#[derive(Default)]` により `PostFormInput::default()` で全部空の値を作れる。`delete_image` は編集画面用で、今回は使わない。

検証済みの型はフィールドに `pub` が無い。

```rust:src/validators/post_form.rs
/// 検証済みの画像。中身のバイト列が jpg / gif / png のどれかであることを確認済み。
#[derive(Debug, Clone)]
pub struct ValidatedImage {
    bytes: Vec<u8>,
    extension: ImageExt,
}

// ...（省略：bytes() -> &[u8] と extension() -> ImageExt の読み取りメソッド）

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

    // ...（省略：text() / image() / delete_image() も同じ形）
}
```

Rust のフィールドは、`pub` を付けない限り定義したモジュールの中からしか見えない。外で `ValidatedPostForm { ... }` と書くとコンパイルエラーになるので、この型を作れるのは `post_form.rs` の `validate` だけだ。保存する処理がこの型を受け取れば、検証を飛ばしたコードはコンパイルが通らない。**検証した後のコードは型を信用してよく、同じ検査を繰り返さなくて済む**。

中身は `title()` のようなメソッドで貸し出す。戻り値の `&str` や `&[u8]` は借用で、最大 5MB の画像でも複製が起きない。タスク文書の型定義はフィールドがすべて `pub` だったが、プロジェクトは「`validate` だけが作れる」を守るために非公開にした。

## `ImageExt` で許可する形式を3つに固定する

拡張子は文字列ではなく列挙型で持つ。`src/models/image_ext.rs` に置く。

```rust:src/models/image_ext.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageExt {
    Jpg,
    Gif,
    Png,
}

impl ImageExt {
    // ...（省略：ドキュメントコメント）
    pub fn as_str(self) -> &'static str {
        match self {
            ImageExt::Jpg => "jpg",
            ImageExt::Gif => "gif",
            ImageExt::Png => "png",
        }
    }

    pub fn from_mime(mime: &str) -> Option<Self> {
        match mime {
            "image/jpeg" => Some(ImageExt::Jpg),
            "image/gif" => Some(ImageExt::Gif),
            "image/png" => Some(ImageExt::Png),
            _ => None,
        }
    }
}
```

`ImageExt` の値は3つのどれかにしかなれず、`as_str()` が返すのも3つの固定文字列だけだ。次回、画像を `{id}.{拡張子}` で保存するときは拡張子にこれだけを使う。`../../etc/passwd` という名前で送られても、保存名は `3.png` のようになる（NFR-103）。

`as_str` が `self` を値で取れるのは、`ImageExt` が `Copy` で、値渡ししても元の変数が使えなくならないからだ。`&'static str` は「プログラム終了まで有効な文字列への参照」で、文字列リテラルの型だ。

## `validate` を書く

```rust:src/validators/post_form.rs
const TITLE_MAX_CHARS: usize = 50;
const TEXT_MAX_CHARS: usize = 500;

/// 投稿フォームの入力を検証する。
// ...（省略：戻り値と学習メモのドキュメントコメント）
pub fn validate(input: &PostFormInput, config: &AppConfig) -> AppResult<ValidatedPostForm> {
    let mut errors: Vec<String> = Vec::new();

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
            // ...（省略：コメント）
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
        title: title.to_string(),
        text: input.text.clone(),
        image,
        delete_image: input.delete_image,
    })
}
```

上限の 5MB と許可する MIME タイプは、第4回の `AppConfig` から読む。`input` も `config` も借用なので、呼んだ後も呼び出し元は両方を使える。この性質は 400 の画面を作るときに効いてくる。

### 文字数は `chars().count()` で数える

Rust の `String` は UTF-8 のバイト列で、`len()` が返すのは **バイト数** だ。ひらがなは1文字3バイトなので、`"あ".repeat(50)` の `len()` は 150 になる。`len()` で比べると、日本語は17文字（51バイト）で上限を超えてしまう。`chars()` は1文字（Unicode のコードポイント）ずつ取り出すイテレータで、`.count()` でその数を数える。

タイトルは `trim()` で前後の空白を落としてから数え、空白だけなら「空」とする。`trim()` は元の文字列の一部を指す `&str` を返すだけで複製を作らない。保存用の `String` は、合格したときに `to_string()` で初めて作る。

### 画像の形式は先頭バイトで判定する

ファイル名の拡張子とクライアントが送る `Content-Type` は、送り手が自由に書ける。だから中身を見る。画像の多くは先頭に決まったバイト列（**マジックナンバー**）を持つ。PNG は `89 50 4E 47`、JPEG は `FF D8 FF` で始まる。これで形式を当てるのが `infer` クレートだ（NFR-102）。

```toml:Cargo.toml
# ...（省略：コメント）
infer = "0.16"
```

`infer::get(&[u8])` は `Option<infer::Type>` を返し、判定できなければ `None` だ。`Type::mime_type()` は `"image/png"` のような `&'static str` を返す。`validate` ではこれに `Option` のメソッドを3つつないでいる。

- `map` は `Some(x)` の中身に関数を当てる。`None` は `None` のまま
- `filter` は条件が偽なら `None` にする。ここでは `allowed_mime` に無い MIME を落とす
- `and_then` は `Option` を返す関数をつなぐ。`ImageExt::from_mime` が許可外で返す `None` がそのまま結果になる

どこかで `None` になれば最後まで `None` が流れるので、3つの条件を `if` の入れ子なしに1つの式で書ける。`infer::Type` には拡張子を返す `extension()` もあるが、拡張子を `&str` ではなく「3つのどれか」の型で持ちたいので使っていない。

### エラーは `Vec<String>` に集める

各検査は失敗しても `return` せず、メッセージを `errors` に `push` して次へ進む。`push` で中身を変えるので `let mut` が要る。最後に `errors` が空でなければ、まとめて `Err(AppError::Validation(errors))` で返す。タイトルを直したら今度は画像で弾かれる、という往復をさせないためだ。

`AppError` に足した `Validation(Vec<String>)` は、`into_response` で `(StatusCode::BAD_REQUEST, messages.join("\n"))` になる。第6回の `Sqlx` は内部情報が漏れないよう決まった文言だけを返したが、こちらはコードに書いた固定文なのでそのまま見せてよい。

タスク文書では戻り値が `Result<ValidatedPostForm, Vec<String>>` だった。プロジェクトは共通の `AppResult` に揃え、ハンドラが他のエラーと同じ `?` の流れで扱えるようにした。タスク文書が前倒しで入れるとしていた `thiserror` は、実際に使う TASK-0012（第10回）で入る。

### 0バイトの画像は拒否する

タスク文書のスケッチは空のバイト列を「画像なし」として通していたが、プロジェクトは要件 EDGE-103 に従って「画像ファイルが空です」にした。ただしブラウザは、ファイル未選択でも空のパートを送ってくる。この見分けはハンドラ側で付ける。

## バリデータのテスト

`validate` は DB も HTTP も使わない純粋な関数なので、普通の `#[test]` で境界値を確かめられる。17件から1件を抜き出す。

```rust:src/validators/post_form.rs
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
```

`input` と `errors_of` はテスト用のヘルパだ。3つの検査がすべて失敗する入力で、メッセージが3つ順に並ぶことを確かめている。

```sh
$ cargo test validators
```

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.21s
     Running unittests src/lib.rs (target/debug/deps/rust_axum_image_bbs-483e4e03c117b98b)

running 17 tests
test validators::post_form::tests::empty_image_returns_error ... ok
test validators::post_form::tests::all_errors_are_returned_together ... ok
test validators::post_form::tests::empty_text_without_image_is_ok ... ok
test validators::post_form::tests::extension_comes_from_content_not_file_name ... ok
test validators::post_form::tests::image_ext_strings_are_fixed ... ok
test validators::post_form::tests::image_invalid_mime_returns_error ... ok
test validators::post_form::tests::image_jpeg_magic_bytes_is_ok ... ok
test validators::post_form::tests::pdf_named_like_an_image_returns_error ... ok
test validators::post_form::tests::text_500_chars_is_ok ... ok
test validators::post_form::tests::text_501_chars_returns_error ... ok
test validators::post_form::tests::title_50_chars_is_ok ... ok
test validators::post_form::tests::title_51_chars_returns_error ... ok
test validators::post_form::tests::title_empty_returns_error ... ok
test validators::post_form::tests::title_is_trimmed ... ok
test validators::post_form::tests::title_with_only_spaces_returns_error ... ok
test validators::post_form::tests::image_over_5mb_returns_error ... ok
test validators::post_form::tests::image_of_exactly_5mb_is_ok ... ok

test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out; finished in 0.12s

     Running unittests src/main.rs (target/debug/deps/rust_axum_image_bbs-8c2eeb3d4fefb7e6)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/integration_create.rs (target/debug/deps/integration_create-512533f368a5eaba)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.00s

     Running tests/integration_list.rs (target/debug/deps/integration_list-8087344ae17c8983)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out; finished in 0.00s
```

文字数は日本語50文字と51文字、画像は 5MB と 5MB + 1バイトで、境目を両側から押さえている。中身が PDF で名前が `evil.jpg` のファイルを拒否する `pdf_named_like_an_image_returns_error` もある。

## `POST /posts` のハンドラを書く

ここから TASK-0010 だ。`Multipart` 抽出器は axum の `multipart` feature で有効になる。

```toml:Cargo.toml
axum = { version = "0.7", features = ["multipart"] }
```

`build_app` に `.route("/posts", post(posts::create))` を足す。`post(...)` は POST メソッドだけをこのハンドラに回す。

### `create` ハンドラ

```rust:src/handlers/posts.rs
/// `POST /posts` 投稿を作成する（multipart/form-data）。
// ...（省略：流れと学習メモ。本文で説明する）
pub async fn create(
    State(state): State<AppState>,
    multipart: Result<Multipart, MultipartRejection>,
) -> AppResult<Response> {
    let mut multipart = multipart?;
    let input = parse_multipart(&mut multipart).await?;

    let form = match validate(&input, &state.config) {
        Ok(form) => form,
        Err(AppError::Validation(errors)) => {
            let view = IndexView {
                form: input,
                errors,
                ..index_view(&state, 1).await?
            };
            return Ok((StatusCode::BAD_REQUEST, view).into_response());
        }
        Err(err) => return Err(err),
    };

    // 画像の保存は TASK-0011 で追加する。ここではタイトルと本文だけを保存する
    post_repo::insert(&state.db, form.title(), form.text()).await?;

    Ok(Redirect::to(CREATED_REDIRECT).into_response())
}
```

流れは「multipart を読む → 検証する → INSERT して 303」だ。最後の `Err(err)` の腕は今は来ないが、`AppError` には `Sqlx` などもあり、`match` は全部の場合を書かないとコンパイルが通らない。303 と 400 の HTML は型が違うので、`.into_response()` でどちらも `Response` にして揃えている。

`Multipart` は引数の最後に置く。axum のドキュメントによると、ボディは一度しか読めない非同期のストリームなので、ボディを読む抽出器は1つだけ、しかも最後の引数でなければならない。

### `Result<Multipart, MultipartRejection>` で受ける理由

`Multipart` 抽出器は、`Content-Type` が `multipart/form-data` でないと失敗する。引数を `Multipart` とだけ書くと、axum はハンドラを呼ばずに既定の 400 を返す。本文は英語の `` Invalid `boundary` for `multipart/form-data` request `` だ。

抽出器を `Result<T, T::Rejection>` で包むと、失敗してもハンドラが呼ばれ、`Err(拒否理由)` が渡る。axum-core 0.4.5 の実装は、中の結果を `Ok` に包むだけだ。

```rust:axum-core-0.4.5/src/extract/mod.rs
    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        Ok(T::from_request(req, state).await)
    }
```

第7回の `Option<Query<T>>` と違い、拒否理由を受け取れる。冒頭の `multipart?` が `Err` を `AppError` に変えて return する。`?` は `From` トレイトで変換するので、`errors.rs` に変換を2つ足した。

```rust:src/errors.rs
const FORM_UNREADABLE: &str = "フォームの送信内容を読み取れませんでした";

/// multipart の読み取り途中の失敗 → `AppError`。
impl From<MultipartError> for AppError {
    fn from(err: MultipartError) -> Self {
        if err.status() == StatusCode::PAYLOAD_TOO_LARGE {
            AppError::PayloadTooLarge
        } else {
            AppError::Validation(vec![FORM_UNREADABLE.to_string()])
        }
    }
}

/// `Multipart` 抽出器そのものの失敗（Content-Type が multipart/form-data でない等）→ 400。
// ...（省略：学習メモ）
impl From<MultipartRejection> for AppError {
    fn from(_err: MultipartRejection) -> Self {
        AppError::Validation(vec![FORM_UNREADABLE.to_string()])
    }
}
```

どちらも日本語のメッセージに揃え、上限超過だけを 413 に分けた。`_err` の `_` は「使わない」印だ。

### `parse_multipart` で欄を1つずつ読む

```rust:src/handlers/posts.rs
// ...（省略：ドキュメントコメント）
async fn parse_multipart(multipart: &mut Multipart) -> AppResult<PostFormInput> {
    let mut input = PostFormInput::default();

    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("title") => input.title = field.text().await?,
            // ブラウザは textarea の改行を CRLF（2文字）で送ってくる。
            // 画面の maxlength は改行を1文字と数えるので、そろえるために LF に直しておく
            Some("text") => input.text = field.text().await?.replace("\r\n", "\n"),
            Some("image") => {
                let filename = field.file_name().unwrap_or_default().to_owned();
                let bytes = field.bytes().await?;
                if !(filename.is_empty() && bytes.is_empty()) {
                    input.image_bytes = Some(bytes.to_vec());
                    input.image_filename = Some(filename);
                }
            }
            Some("delete_image") => input.delete_image = true,
            _ => {}
        }
    }

    Ok(input)
}
```

multipart のボディは欄ごとの「パート」の並びだ。`next_field()` は次のパートを返し、最後で `Ok(None)` になる。`while let Some(field) = ...` は `Some` の間だけ回るループだ。

`field.name()` は名前を借りるだけだが、`text()` と `bytes()` は `self` を値で受け取り、フィールドを消費する。だから先に名前で振り分けてから中身を読む。`image` の腕では、消費の前にファイル名を `to_owned()` で自分の `String` に複製している。

ファイル欄を空のまま送信すると、ブラウザは `filename=""` で中身が空のパートを送る。0バイトの `empty.jpg` を選べば `filename="empty.jpg"` で中身が空だ。ファイル名も中身も空のときだけ「画像なし」とし、それ以外はバリデータに渡す。バリデータは要件どおり「空のファイルは拒否」だけを書けばよい。

## 検証に落ちたら入力を残して 400 で描き直す

要件 REQ-201 は「入力値を保持したまま、エラーメッセージとともにフォームを再表示する」だ。旧 PHP 版は入力を保持しなかったので、ここは改善点になる。

`IndexView` にフィールドを3つ足す。

```rust:src/templates/mod.rs
    pub form: PostFormInput,
    pub errors: Vec<String>,
    pub success_msg: Option<&'static str>,
```

`form` は失敗した生の入力、`errors` はメッセージ一覧、`success_msg` は後で見る成功メッセージだ。一覧とフォームは同じ画面なので、400 でも一覧を組み立て直す。前回 `list` にあったページ計算と DB の読み出しは、関数 `index_view(state: &AppState, requested_page: u32)` に切り出した。

### 構造体更新記法で一部だけ差し替える

`create` の失敗側の `IndexView { form: input, errors, ..index_view(&state, 1).await? }` の `..` は **構造体更新記法** で、「書いたフィールド以外は `..` の後ろの値から持ってくる」という意味だ。`index_view` が返す1ページ目の `IndexView` から、`form` と `errors` だけを差し替える。`errors` は `errors: errors` の省略形だ。

`form: input` で、`input` の所有権は `IndexView` へ移る（ムーブ）。`validate(&input, ...)` が借用だったので、この時点でも `input` は手元にある。multipart を読み直して入力を取り戻すことはできない。ボディはストリームで、`parse_multipart` がもう読み切っているからだ。

最後の `(StatusCode::BAD_REQUEST, view)` は、ステータスと `IntoResponse` な値のタプルだ。axum はこれもレスポンスにできるので、`IndexView` の HTML に 400 が付く。

### テンプレートに入力値とエラーを流し込む

```html:templates/index.html
{% if !errors.is_empty() %}
<div class="alert alert-danger" role="alert">
  <ul class="mb-0">
    {% for e in errors %}<li>{{ e }}</li>{% endfor %}
  </ul>
</div>
{% endif %}

<form method="post" action="/posts" enctype="multipart/form-data" class="card card-body mb-4">
  <div class="mb-3">
    <label for="title" class="form-label">タイトル</label>
    <input id="title" name="title" class="form-control" maxlength="50" value="{{ form.title }}">
  </div>
  <div class="mb-3">
    <label for="text" class="form-label">本文</label>
    <textarea id="text" name="text" class="form-control" rows="4" maxlength="500">{{ form.text }}</textarea>
  </div>
```

エラーが無ければ赤い枠ごと出ない。`{{ }}` の中身は第6回で見たとおり HTML エスケープされるので、タイトルに `"><script>` と書かれても `value` 属性から抜け出せない（統合テスト `rerendered_input_is_html_escaped`）。画像欄は戻せない。`<input type="file">` にはサーバから値を入れられないので、画像は選び直してもらう。

## 成功したら 303 で一覧へ戻す

### PRG パターン

POST の応答で HTML を返すと、再読み込みで同じ POST が送り直され、二重投稿になる。POST の応答はリダイレクトだけにして、一覧は GET で取りに行かせる。Post / Redirect / Get の頭文字で **PRG パターン** と呼ぶ。

axum 0.7 の `Redirect::to` は 303 See Other を返し、次のリクエストを GET に変えさせる。axum のドキュメントも、フォーム送信やファイルアップロードの成功後に向くと説明している。`Redirect::temporary`（307）は元のメソッドを保つので、`/` に POST を送り直させてしまう。

### 成功メッセージは種類だけを URL に載せる

要件 REQ-202 は「投稿成功直後に成功メッセージを表示する」だ。リダイレクト先の GET は別のリクエストなので、成功を URL で伝える。リダイレクト先は定数 `CREATED_REDIRECT = "/?notice=created"` で、`notice` は列挙型で読む。

```rust:src/handlers/posts.rs
#[derive(Debug, Deserialize)]
pub struct NoticeQuery {
    pub notice: Notice,
}

/// 一覧の上に出す成功メッセージの種類。
// ...（省略：学習メモ）
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Notice {
    Created,
}

impl Notice {
    fn message(self) -> &'static str {
        match self {
            Notice::Created => "投稿しました",
        }
    }
}
```

省略した学習メモによると、文面を `?msg=投稿しました` のように URL で受け取ると、誰でも好きな文面を出すリンクを作れてしまう。エスケープでスクリプトは防げても、偽の告知は防げない。`rename_all = "snake_case"` で `Created` は URL 上で `created` になり、serde は列挙型にある値しか受け付けない。画面に出すのは `message()` の固定文だ。**URL の文字列がそのまま画面に出る経路が無い**。`success_msg` を `Option<&'static str>` にしたのも、コードに書いた文字列しか入らない型にするためだ。

一覧のハンドラは引数に `notice: Option<Query<NoticeQuery>>` を足し、`index_view` の結果に構造体更新記法でメッセージだけを入れる。

```rust:src/handlers/posts.rs
    Ok(IndexView {
        success_msg: notice.map(|Query(q)| q.notice.message()),
        ..view
    })
```

`?page` 用の `PaginationQuery` と `NoticeQuery` は、1つのクエリ文字列を別々に読み、自分に関係ないキーは無視する。`notice` が無いか一覧に無い値なら `None` で、メッセージは出ない。

## ボディの上限を「画像 + 64KiB」にする

第4回の `DefaultBodyLimit::max(max_body_bytes)` は、画像の上限そのもの（5MB）を使い、「`Multipart` を使うハンドラで働き始める」と書いた。今回その値を変える。

```rust:src/lib.rs
/// リクエストボディの上限を決めるとき、画像の上限（`max_image_bytes`）に上乗せする余裕（64KiB）。
///
/// multipart のボディには画像のほかに、区切り線・各欄のヘッダ・タイトルと本文の値が入る。
/// 上限を画像の上限ちょうどにすると、ちょうど 5MB の画像（受け付けるべきもの）まで 413 で弾かれてしまう。
/// - 画像 ≤ 5MB              → 受け付ける
/// - 5MB < 画像 ≤ 5MB + 余裕 → ボディは読めるのでバリデータまで届き、フォーム再表示の 400
/// - それより大きい          → 読み取りの途中で打ち切って 413（巨大なボディをメモリに溜めない）
const MULTIPART_OVERHEAD_BYTES: usize = 64 * 1024;
```

```rust:src/lib.rs
    let max_body_bytes = state.config.max_image_bytes + MULTIPART_OVERHEAD_BYTES;
```

上限は 5,242,880 + 65,536 = 5,308,416 バイトになる。axum 0.7.9 の `Multipart` は、ボディを `http_body_util::Limited` で包んでから読む。上限を超えた時点で読み取りが失敗し、`MultipartError::status()` が 413 を返す。それが `From<MultipartError>` で `AppError::PayloadTooLarge` になり、413 と「画像サイズは5MB以内にしてください」の文字だけが返る。ボディを読み切っていないので入力が手元に無く、フォームは描き直せない。

## `post_repo::insert` で INSERT する

```rust:src/repository/post_repo.rs
/// 画像なしで投稿を1件 INSERT し、採番された id を返す。
///
/// 画像は id が決まってから `{id}.{拡張子}` の名前で保存するので、INSERT の時点では NULL のまま。
/// `regist_date` / `update_date` はテーブル定義の `DEFAULT (datetime('now'))` に任せる。
pub async fn insert(db: &SqlitePool, title: &str, text: &str) -> AppResult<i64> {
    let result = sqlx::query("INSERT INTO post (title, text) VALUES (?, ?)")
        .bind(title)
        .bind(text)
        .execute(db)
        .await?;
    Ok(result.last_insert_rowid())
}
```

`last_insert_rowid()` は採番された id を返す。id は今回は使わず、次回画像の保存名に使う。

タスク文書は `sqlx::query!` マクロで、日時も SQL に書く案だった。プロジェクトは第5回で決めたとおり、ビルド時に DB が要らない実行時 API の `sqlx::query` を使い、日時はテーブル定義の `DEFAULT` に任せた。マクロの検証の代わりに、テスト `test_insert_returns_new_id_without_image` が実際の SQLite で確かめている。

## 統合テストで multipart を送る

`tests/common/mod.rs` に、multipart のボディを組み立てて送る `post_multipart` と、行数を数える `post_count` を足した。テストは `tests/integration_create.rs` に12件ある。1件を抜き出す。

```rust:tests/integration_create.rs
#[tokio::test]
async fn japanese_title_of_50_chars_is_accepted() {
    let app = common::setup().await;
    let title = "あ".repeat(50);

    let res = common::post_multipart(&app.router, "/posts", &[Part::Text("title", &title)]).await;

    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(common::post_count(&app.db).await, 1);
}
```

バリデータのテストと同じ境界を、HTTP 越しに確かめている。

```sh
$ cargo test --test integration_create
```

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.16s
     Running tests/integration_create.rs (target/debug/deps/integration_create-512533f368a5eaba)

running 12 tests
test broken_multipart_returns_400 ... ok
test create_inserts_post_and_redirects_with_303 ... ok
test empty_file_part_from_browser_means_no_image ... ok
test japanese_title_of_50_chars_is_accepted ... ok
test empty_title_rerenders_form_with_input_kept ... ok
test non_multipart_request_returns_400 ... ok
test rerendered_input_is_html_escaped ... ok
test success_message_is_shown_only_after_create ... ok
test zero_byte_image_file_is_rejected ... ok
test text_of_501_chars_is_rejected_and_title_is_kept ... ok
test body_far_over_the_limit_returns_413 ... ok
test image_just_over_5mb_is_a_validation_error ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
```

ほかに、400 の画面に入力値と一覧が残ること、「ファイル未選択」と「0バイトのファイル」の見分け、5MB + 1バイトの 400 と 6MB の 413 も確かめている。

## 動かして確かめる

DB を消してから起動する。

```sh
$ rm -f data/app.db
$ cargo run
```

```
2026-11-17T14:45:41.079014Z  INFO rust_axum_image_bbs: migrations applied
2026-11-17T14:45:41.079250Z  INFO rust_axum_image_bbs: server listening on http://0.0.0.0:3000
```

別のターミナルから `curl -F` で multipart を送る。`-F 'name=@ファイル'` はファイルの添付だ。ヘッダと HTML は関係する行だけ残し、ほかは `...` で省いた。

### テキストだけの投稿

```sh
$ curl -i -F 'title=はじめての投稿' -F 'text=フォームから送った本文' http://localhost:3000/posts
HTTP/1.1 303 See Other
location: /?notice=created
content-length: 0
date: Tue, 17 Nov 2026 14:45:41 GMT

$ curl -i 'http://localhost:3000/?notice=created'
HTTP/1.1 200 OK
...
<div class="alert alert-success" role="status">投稿しました</div>
...
          <h3 class="h5 card-title">はじめての投稿</h3>
          <p class="card-text">フォームから送った本文</p>
...
```

303 の後、リダイレクト先に「投稿しました」と1件目が出た。

### タイトルが空

```sh
$ curl -i -F 'title=' -F 'text=本文だけ書いた' http://localhost:3000/posts
HTTP/1.1 400 Bad Request
content-type: text/html; charset=utf-8
...
<div class="alert alert-danger" role="alert">
  <ul class="mb-0">
    <li>タイトルは必須です</li>
  </ul>
</div>
...
    <textarea id="text" name="text" class="form-control" rows="4" maxlength="500">本文だけ書いた</textarea>
...
          <h3 class="h5 card-title">はじめての投稿</h3>
...
```

400 で、エラーの枠と、入力した本文と、一覧が返った。

### 51文字と50文字の日本語タイトル

```sh
$ curl -i -F "title=$(python3 -c 'print("あ" * 51)')" http://localhost:3000/posts
HTTP/1.1 400 Bad Request
...
    <li>タイトルは50文字以内で入力してください</li>
...
$ curl -i -F "title=$(python3 -c 'print("あ" * 50)')" http://localhost:3000/posts
HTTP/1.1 303 See Other
location: /?notice=created
...
```

「あ」50個は150バイトある。`len()` で数えていたら、ここも 400 だった。

### テキストファイルを画像として送る

`memo.txt` の中身は「これは画像ではない」の1行だ。`;type=image/png;filename=photo.png` で、`Content-Type` とファイル名を PNG に偽装する。

```sh
$ curl -i -F 'title=画像のつもり' -F 'image=@memo.txt;type=image/png;filename=photo.png' http://localhost:3000/posts
HTTP/1.1 400 Bad Request
...
    <li>対応していない画像形式です (jpg/gif/png)</li>
...
    <input id="title" name="title" class="form-control" maxlength="50" value="画像のつもり">
...
```

中身の先頭が PNG のマジックナンバーではないので弾かれ、タイトルは残った。

### 5MB ちょうど・5MB + 1バイト・6MB

PNG のマジックナンバーの後ろを 0 で埋めて、3つのファイルを作る。

```sh
$ python3 - <<'EOF'
magic=b"\x89PNG\r\n\x1a\n"
for name,n in [("5mb.png",5*1024*1024),("5mb_plus1.png",5*1024*1024+1),("6mb.png",6*1024*1024)]:
    open(name,"wb").write(magic+b"\0"*(n-len(magic)))
EOF
```

大きいボディでは curl が `Expect: 100-continue` を付けてサーバの返事を待つので、先に `100 Continue` が出る。

```sh
$ curl -i -F 'title=5MB ちょうど' -F 'image=@5mb.png' http://localhost:3000/posts
HTTP/1.1 100 Continue

HTTP/1.1 303 See Other
location: /?notice=created
...
$ curl -i -F 'title=5MB + 1 バイト' -F 'image=@5mb_plus1.png' http://localhost:3000/posts
HTTP/1.1 100 Continue

HTTP/1.1 400 Bad Request
...
    <li>画像サイズは5MB以内にしてください</li>
...
$ curl -i -F 'title=6MB' -F 'image=@6mb.png' http://localhost:3000/posts
HTTP/1.1 100 Continue

HTTP/1.1 413 Payload Too Large
content-type: text/plain; charset=utf-8
content-length: 48
date: Tue, 17 Nov 2026 14:45:41 GMT

画像サイズは5MB以内にしてください
```

5MB ちょうどは 303 で受け付けた。1バイト多いとバリデータまで届き、フォームが 400 で返った。6MB は上限の 5,308,416 バイトを超え、読み取りの途中で打ち切られて文字だけの 413 になった。

### ファイル未選択

`;filename=` でファイル名を空にすると、ブラウザがファイル未選択で送るのと同じ `filename=""` のパートになる。中身は0バイトだが「画像なし」として通る。

```sh
$ : > empty.jpg
$ curl -i -F 'title=画像なし' -F 'image=@empty.jpg;filename=' http://localhost:3000/posts
HTTP/1.1 303 See Other
...
```

### 画像はまだ保存されない

```sh
$ python3 - data/app.db <<'EOF'
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
for row in con.execute("SELECT id, length(title), title, image FROM post"):
    print(row)
EOF
(1, 7, 'はじめての投稿', None)
(2, 50, 'ああああああああああああああああああああああああああああああああああああああああああああああああああ', None)
(3, 8, '5MB ちょうど', None)
(4, 4, '画像なし', None)
```

成功した4件だけが入った。3件目は画像付きで成功したが、`image` は NULL で、`UPLOAD_DIR` も空のままだった。

## 詰まったポイントと気づき

- **`len()` はバイト数**: 日本語は17文字で51バイトになる。文字数は `chars().count()` で数える
- **上限を画像の上限と同じにすると、5MB ちょうどの画像が 413 になる**: multipart には区切り線や他の欄のぶんがあるので、「画像 + 64KiB」にした
- **ブラウザはファイル未選択でも空のパートを送る**: そのまま0バイトの画像として拒否すると、画像なしで投稿できない
- **multipart は読み直せない**: 400 で入力を戻すには、読んだ `PostFormInput` を残すしかない
- **`<textarea>` の改行は CRLF で届く**: 画面の `maxlength` は改行を1文字と数えるので、LF に直してから検証する
- **413 ではフォームを描き直せない**: ボディを読み切っていないので入力が手元に無い

## 次回予告

第9回は、今回検証だけして捨てていた画像を保存する。

- 検証済みの画像を `{id}.{拡張子}` の名前で `UPLOAD_DIR` に保存する。拡張子は `ImageExt::as_str()` だけから作る
- `tower-http` の `ServeDir` で、保存した画像を `/uploads` の下で配信する
- 画像の保存に失敗したら、先に INSERT した行を削除して投稿を取り消す

今回の `post_repo::insert` が返す id は、そこで使う。

- 次回: 「Rust + axum で画像掲示板を作る (9) 画像アップロードとロールバック」

## 参考リンク

- [axum 0.7 `Multipart` (docs.rs)](https://docs.rs/axum/0.7/axum/extract/struct.Multipart.html)
- [axum 0.7 extract: The order of extractors](https://docs.rs/axum/0.7/axum/extract/index.html#the-order-of-extractors) / [Optional extractors](https://docs.rs/axum/0.7/axum/extract/index.html#optional-extractors) — `Result<T, T::Rejection>` で受ける書き方
- [axum 0.7 `DefaultBodyLimit`](https://docs.rs/axum/0.7/axum/extract/struct.DefaultBodyLimit.html)
- [axum 0.7 `Redirect`](https://docs.rs/axum/0.7/axum/response/struct.Redirect.html)
- [infer 0.16 (docs.rs)](https://docs.rs/infer/0.16/infer/)
- [`str::chars`](https://doc.rust-lang.org/std/primitive.str.html#method.chars)
- [serde: Container attributes](https://serde.rs/container-attrs.html) — `#[serde(rename_all = "...")]`
- [askama 0.12 (docs.rs)](https://docs.rs/askama/0.12/askama/)
- [MDN: 303 See Other](https://developer.mozilla.org/ja/docs/Web/HTTP/Status/303)
