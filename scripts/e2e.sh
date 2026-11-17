#!/usr/bin/env bash
# 実サーバに対する end-to-end 確認スクリプト。
#
# 使い方:
#   scripts/e2e.sh          # 投稿作成・一覧・ページング・編集・削除をすべて確認する
#   scripts/e2e.sh create   # 投稿作成・一覧・ページングまで（編集・削除を実装する前の段階用）
#
# やっていること:
#   1. cargo build でバイナリを作る
#   2. 一時ディレクトリに DB とアップロード先を作り、空きポートでサーバを起動する
#   3. curl で実際に HTTP リクエストを送り、ステータス・HTML・保存された画像を確かめる
#   4. 終了時にサーバを止め、一時ディレクトリを消す
#
# cargo test の統合テストはルーターを直接叩く（ポートを開かない）。
# このスクリプトは「本当に起動したサーバ」を外から叩くので、起動処理・静的配信・
# ボディサイズ上限など、プロセス全体の配線ミスに気づける。
set -euo pipefail

UPTO="${1:-all}"
PORT="${E2E_PORT:-3999}"
BASE="http://127.0.0.1:${PORT}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
PASS=0
FAIL=0

cleanup() {
    if [[ -n "${SERVER_PID:-}" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT

ok() { PASS=$((PASS + 1)); echo "  ok   $1"; }
ng() { FAIL=$((FAIL + 1)); echo "  FAIL $1"; }

# check <説明> <期待値> <実際の値>
check() {
    if [[ "$2" == "$3" ]]; then ok "$1"; else ng "$1 (expected: $2, actual: $3)"; fi
}
# contains <説明> <ファイル> <含まれるべき文字列>
contains() {
    if grep -qF -- "$3" "$2"; then ok "$1"; else ng "$1 (not found: $3)"; fi
}
# lacks <説明> <ファイル> <含まれてはいけない文字列>
lacks() {
    if grep -qF -- "$3" "$2"; then ng "$1 (found: $3)"; else ok "$1"; fi
}
# status <curl の引数...> : ステータスコードだけを表示し、本文は $WORK/body に保存する
status() {
    curl -s -o "$WORK/body" -w '%{http_code}' "$@"
}
# newest_id : 一覧の先頭（最新）の投稿 ID を編集リンクから取り出す（編集機能の実装後に使える）
newest_id() {
    curl -s "$BASE/" | grep -oE '/posts/[0-9]+/edit' | head -1 | grep -oE '[0-9]+' || true
}
# newest_upload : 一覧の先頭にある画像のパス（例: /uploads/3.png）を取り出す
newest_upload() {
    curl -s "$BASE/" | grep -oE '/uploads/[0-9]+\.(png|gif|jpg)' | head -1 || true
}
# repeat <文字> <回数>
repeat() {
    printf "%0.s$1" $(seq 1 "$2")
}

# ---------- テスト用ファイル ----------
# 1x1 ピクセルの本物の画像。サーバは拡張子ではなくファイル先頭のバイト列で形式を判定する。
base64 -d > "$WORK/a.png" <<'EOF'
iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==
EOF
base64 -d > "$WORK/a.gif" <<'EOF'
R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7
EOF
base64 -d > "$WORK/a.jpg" <<'EOF'
/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=
EOF
echo "this is not an image" > "$WORK/fake.png"
head -c $((6 * 1024 * 1024)) /dev/zero > "$WORK/big.png"

# ---------- サーバ起動 ----------
echo "== build"
(cd "$ROOT" && cargo build --quiet)

echo "== start server on $BASE"
(
    cd "$ROOT"
    PORT="$PORT" DATABASE_URL="sqlite://$WORK/app.db" UPLOAD_DIR="$WORK/uploads" \
        RUST_LOG="${RUST_LOG:-warn}" exec ./target/debug/rust-axum-image-bbs
) > "$WORK/server.log" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 50); do
    if curl -s -o /dev/null "$BASE/"; then break; fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        echo "server exited early:"; cat "$WORK/server.log"; exit 1
    fi
    sleep 0.2
done

# ---------- 一覧（空） ----------
echo "== list (empty)"
check "GET / is 200" 200 "$(status "$BASE/")"
contains "page has the post form" "$WORK/body" 'name="title"'
contains "form is multipart" "$WORK/body" 'multipart/form-data'

# ---------- 投稿作成 ----------
echo "== create"
check "POST /posts (text only) is 303" 303 "$(status -F 'title=first post' -F 'text=hello body' "$BASE/posts")"
check "GET / is 200" 200 "$(status "$BASE/")"
contains "list shows the new title" "$WORK/body" 'first post'
contains "list shows the new text" "$WORK/body" 'hello body'

check "POST /posts with png is 303" 303 "$(status -F 'title=with png' -F "image=@$WORK/a.png" "$BASE/posts")"
UPLOAD="$(newest_upload)"
check "list links the saved image as .png" png "${UPLOAD##*.}"
check "GET /uploads/{id}.png is 200" 200 "$(status "$BASE$UPLOAD")"
if cmp -s "$WORK/body" "$WORK/a.png"; then ok "served image equals the upload"; else ng "served image differs from the upload"; fi

check "POST /posts with gif is 303" 303 "$(status -F 'title=with gif' -F "image=@$WORK/a.gif" "$BASE/posts")"
UPLOAD="$(newest_upload)"
check "list links the saved image as .gif" gif "${UPLOAD##*.}"
check "GET /uploads/{id}.gif is 200" 200 "$(status "$BASE$UPLOAD")"
# 拡張子を偽っても、中身が JPEG なら .jpg として保存される
check "POST /posts with jpeg named .png is 303" 303 "$(status -F 'title=with jpg' -F "image=@$WORK/a.jpg;filename=evil.png" "$BASE/posts")"
UPLOAD="$(newest_upload)"
check "jpeg content is saved as .jpg" jpg "${UPLOAD##*.}"
check "GET /uploads/{id}.jpg is 200" 200 "$(status "$BASE$UPLOAD")"

# ---------- バリデーション ----------
echo "== validation"
check "empty title is 400" 400 "$(status -F 'title=' -F 'text=keep this text' "$BASE/posts")"
contains "400 page keeps the entered text" "$WORK/body" 'keep this text'
check "51-char title is 400" 400 "$(status -F "title=$(repeat x 51)" "$BASE/posts")"
check "50-char title is 303" 303 "$(status -F "title=$(repeat y 50)" "$BASE/posts")"
check "501-char text is 400" 400 "$(status -F 'title=long text' -F "text=$(repeat z 501)" "$BASE/posts")"
check "500-char text is 303" 303 "$(status -F 'title=max text' -F "text=$(repeat z 500)" "$BASE/posts")"
# 日本語は「文字数」で数える（バイト数ではない）: 50文字 = 150バイト
check "50 Japanese chars title is 303" 303 "$(status -F "title=$(repeat あ 50)" "$BASE/posts")"

check "non-image file is 400" 400 "$(status -F 'title=fake image' -F "image=@$WORK/fake.png" "$BASE/posts")"
check "6MB upload is 413" 413 "$(status -F 'title=too big' -F "image=@$WORK/big.png" "$BASE/posts")"
status "$BASE/" > /dev/null
lacks "rejected non-image post left no row" "$WORK/body" 'fake image'
lacks "rejected 6MB post left no row" "$WORK/body" 'too big'
check "rejected uploads left no file" 3 "$(find "$WORK/uploads" -type f | wc -l | tr -d ' ')"

# ---------- XSS ----------
echo "== escaping"
check "POST script tag title is 303" 303 "$(status --form-string 'title=<script>alert(1)</script>' "$BASE/posts")"
status "$BASE/" > /dev/null
lacks "title is not rendered as raw HTML" "$WORK/body" '<script>alert(1)</script>'
contains "title is HTML-escaped" "$WORK/body" '&lt;script&gt;'

# ---------- ページング ----------
echo "== pagination"
for i in $(seq 1 45); do
    curl -s -o /dev/null -F "title=pg-$(printf '%02d' "$i")" "$BASE/posts"
done
status "$BASE/" > /dev/null
contains "page 1 has the newest post" "$WORK/body" 'pg-45'
lacks "page 1 does not have post 25" "$WORK/body" 'pg-25'
check "page 1 shows 20 posts" 20 "$(grep -oE 'pg-[0-9]{2}' "$WORK/body" | sort -u | wc -l | tr -d ' ')"
status "$BASE/?page=2" > /dev/null
contains "page 2 has post 25" "$WORK/body" 'pg-25'
lacks "page 2 does not have the newest post" "$WORK/body" 'pg-45'
check "GET /?page=abc is 200" 200 "$(status "$BASE/?page=abc")"
contains "invalid page falls back to page 1" "$WORK/body" 'pg-45'
check "GET /?page=0 is 200" 200 "$(status "$BASE/?page=0")"
check "GET /?page=9999 is 200" 200 "$(status "$BASE/?page=9999")"

check "unknown path is 404" 404 "$(status "$BASE/no-such-page")"

if [[ "$UPTO" == "all" ]]; then
    # ---------- 編集 ----------
    echo "== edit"
    check "POST /posts for edit target is 303" 303 "$(status -F 'title=edit me' -F 'text=before' -F "image=@$WORK/a.png" "$BASE/posts")"
    ID="$(newest_id)"
    check "GET /posts/{id}/edit is 200" 200 "$(status "$BASE/posts/$ID/edit")"
    contains "edit form shows the current title" "$WORK/body" 'edit me'
    contains "edit form shows the current text" "$WORK/body" 'before'
    check "GET /posts/999999/edit is 404" 404 "$(status "$BASE/posts/999999/edit")"

    check "edit title and text is 303" 303 "$(status -F 'title=edited' -F 'text=after' "$BASE/posts/$ID/edit")"
    status "$BASE/" > /dev/null
    contains "list shows the edited title" "$WORK/body" 'edited'
    contains "list shows the edited text" "$WORK/body" 'after'
    check "image is kept when no new image is sent" 200 "$(status "$BASE/uploads/$ID.png")"

    check "edit with empty title is 400" 400 "$(status -F 'title=' -F 'text=typed in edit' "$BASE/posts/$ID/edit")"
    contains "400 edit page keeps the entered text" "$WORK/body" 'typed in edit'
    check "edit of a missing post is 404" 404 "$(status -F 'title=x' "$BASE/posts/999999/edit")"

    check "replace png with jpg is 303" 303 "$(status -F 'title=edited' -F "image=@$WORK/a.jpg" "$BASE/posts/$ID/edit")"
    check "new image is served" 200 "$(status "$BASE/uploads/$ID.jpg")"
    check "old image file is removed" 404 "$(status "$BASE/uploads/$ID.png")"

    check "delete_image=on is 303" 303 "$(status -F 'title=edited' -F 'delete_image=on' "$BASE/posts/$ID/edit")"
    check "image file is removed" 404 "$(status "$BASE/uploads/$ID.jpg")"
    status "$BASE/" > /dev/null
    contains "post remains after image delete" "$WORK/body" 'edited'
    lacks "list no longer links the image" "$WORK/body" "/uploads/$ID."

    # ---------- 削除 ----------
    echo "== delete"
    check "POST /posts for delete target is 303" 303 "$(status -F 'title=delete me' -F "image=@$WORK/a.gif" "$BASE/posts")"
    ID="$(newest_id)"
    check "GET /posts/{id}/delete is 405" 405 "$(status "$BASE/posts/$ID/delete")"
    check "POST /posts/{id}/delete is 303" 303 "$(status -X POST "$BASE/posts/$ID/delete")"
    status "$BASE/" > /dev/null
    lacks "deleted post is gone from the list" "$WORK/body" 'delete me'
    check "deleted post's image is removed" 404 "$(status "$BASE/uploads/$ID.gif")"
    check "delete of a missing post is 404" 404 "$(status -X POST "$BASE/posts/$ID/delete")"
fi

echo
echo "passed: $PASS, failed: $FAIL"
if [[ "$FAIL" -ne 0 ]]; then
    echo "--- server log (last 30 lines) ---"
    tail -30 "$WORK/server.log"
    exit 1
fi
