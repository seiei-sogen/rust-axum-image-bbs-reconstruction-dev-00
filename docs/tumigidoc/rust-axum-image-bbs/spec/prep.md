# rust-axum-image-bbs 準備タスク（ユーザー作業）

> **仕様**: [requirements.md](requirements.md)
> **生成日**: 2026-05-22

**【信頼性レベル凡例】**:
- 🔵 **青信号**: 要件定義書・ヒアリングで明確に必要と判明したタスク
- 🟡 **黄信号**: 妥当に推測されるタスク
- 🔴 **赤信号**: 推測による予防的タスク

## 必須（実装開始前に完了が必要）

- [ ] **Rust toolchain のインストール** 🔵 *REQ-401 (axum=Rust必須)*
  - `rustup` で stable 版をインストール（`rustup install stable`）
  - cargo / rustc / rustfmt / clippy が使えること
  - 公式: https://www.rust-lang.org/tools/install
  - 関連要件: 全要件の前提

- [ ] **SQLite3 CLI のインストール** 🔵 *REQ-402*
  - sqlx 経由で接続するが、開発中に直接DBを確認したい場面が多い
  - Linux/WSL: `sudo apt install sqlite3`
  - 関連要件: REQ-402

- [ ] **sqlx-cli のインストール** 🔵 *REQ-402（マイグレーション運用）*
  - `cargo install sqlx-cli --no-default-features --features rustls,sqlite`
  - マイグレーション作成・適用、`cargo sqlx prepare` 用
  - 関連要件: REQ-402, NFR-101

## 推奨（実装中に用意できればOK）

- [ ] **Docker / Docker Compose** 🔵 *NFR-401*
  - WSL から使う場合は Docker Desktop または rootless Docker
  - 必要になるフェーズ: Phase 11（Docker 化ステップ）
  - 関連要件: NFR-401, NFR-403

- [ ] **エディタの rust-analyzer 拡張** 🟡 *学習効率*
  - VSCode 拡張 `rust-analyzer` を有効化（型推論・補完）
  - Zenn 記事のスクリーンショットでも分かりやすい
  - 必要になるフェーズ: 全フェーズ

- [ ] **Zennアカウント / GitHub連携** 🔵 *REQ-408*
  - 各ステップごとに記事化する想定
  - Zenn CLI 利用なら `npm i -g zenn-cli` + リポジトリ初期化
  - 必要になるフェーズ: 各ステップ完了後

- [ ] **旧PHP版の動作確認環境（任意）** 🟡 *リプレイス比較*
  - 旧PHP `old_php_img_bbs/` を MySQL + PHP で動かせると挙動比較がしやすい
  - 必要になるフェーズ: 仕様の細部を確認したい時
  - パス: `/home/agir_wsl/develop/ghq_repos/github.com/seiei-sogen/rust-axum-qwik-imgboard-reconstruction/old_php_img_bbs`

- [ ] **画像テスト用素材の準備** 🟡 *受け入れ基準の境界値テスト*
  - jpg / gif / png / 0byte / 5MB境界 / 6MB / 拡張子偽装 など
  - 必要になるフェーズ: テストフェーズ（cargo test 段階）

## 確認事項（判断が必要）

- [ ] **画像5MBサイズ判定の境界（5MBちょうどは受理するか）** 🟡 *EDGE-001 / TC-004-B05*
  - 「以下」と書いた場合 5,242,880 byte ちょうども受理になる
  - 実装時に `< 5MB` と `<= 5MB` のどちらにするか決定
  - 関連要件: EDGE-001

- [ ] **画像差し替え時の旧拡張子ファイルの扱い** 🟡 *REQ-104*
  - 例: 旧 `5.jpg` → 新 `5.png` で差し替えた時、旧 `5.jpg` を削除するか
  - 推奨: 旧ファイルも削除して `5.png` のみ残す
  - 関連要件: REQ-104

- [ ] **PRG パターン採用可否** 🟡 *REQ-202*
  - POST後にリダイレクトでGETする（リロード再投稿を防ぐ）
  - 旧PHPは同一URLで再表示していたが、Rust版で改善するか
  - 関連要件: REQ-202

- [ ] **空状態メッセージの文言** 🟡 *REQ-101*
  - 「投稿がまだありません」「No posts yet」 等の具体的な文言
  - 関連要件: REQ-101

- [ ] **CSRF 対策の方針** 🔴 *NFR-105 関連の推測*
  - 認証なしのアプリだが、削除・編集の POST に対して CSRF トークンを入れるか
  - 学習目的としてどこまで掘るか要判断
  - 関連要件: NFR-105

---

## サマリー

| 優先度 | 件数 | 🔵 | 🟡 | 🔴 |
|--------|------|-----|-----|-----|
| 必須 | 3 | 3 | 0 | 0 |
| 推奨 | 5 | 2 | 3 | 0 |
| 確認事項 | 5 | 0 | 4 | 1 |
| **合計** | **13** | **5** | **7** | **1** |

## 関連文書

- **要件定義書**: [requirements.md](requirements.md)
- **ヒアリング記録**: [interview-record.md](interview-record.md)
