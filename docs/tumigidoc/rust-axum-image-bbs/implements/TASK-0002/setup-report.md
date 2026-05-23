# TASK-0002 セットアップレポート

- **タスクID**: TASK-0002
- **タスク名**: tracing + tower-http TraceLayer 導入
- **フェーズ**: direct-setup
- **実行日**: 2026-05-23

## 実施内容

### 1. Cargo.toml 更新

`[dependencies]` セクションに以下の3クレートを追加した。

```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tower-http = { version = "0.5", features = ["trace"] }
```

各クレートには初学者向けの日本語コメントを付与し、「なぜ導入するか」を説明した。

### 2. src/main.rs 更新

#### use 宣言の追加

```rust
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;
```

#### subscriber 初期化（main 冒頭）

```rust
tracing_subscriber::fmt()
    .with_env_filter(
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,tower_http=debug")),
    )
    .init();
```

- `RUST_LOG` 未設定時は `info,tower_http=debug` をデフォルト値として使用
- main の最初に配置し、それ以降のすべてのログが確実にキャプチャされるようにした

#### Router への TraceLayer 追加

```rust
let app: Router = Router::new()
    .route("/", get(root_handler))
    .layer(TraceLayer::new_for_http());
```

#### tracing::info! への置換

```rust
tracing::info!("server listening on http://{}", listener.local_addr()?);
```

`println!` から `tracing::info!` へ切り替え、ログフォーマットの統一とレベルフィルタの恩恵を受けられるようにした。

#### 日本語コメント

既存コメントのスタイル（初学者向け丁寧解説）を踏襲し、以下の観点で補足を追加した:

- なぜ `println!` でなく `tracing::info!` を使うのか
- `EnvFilter::try_from_default_env().unwrap_or_else(...)` のエラーハンドリングパターン
- `.layer()` によるミドルウェア積み重ねのイメージ
- `TraceLayer` が自動で何をログ出力するか

## 変更ファイル一覧

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `Cargo.toml` | 追記 | tracing / tracing-subscriber / tower-http を依存追加 |
| `src/main.rs` | 編集 | subscriber 初期化・TraceLayer 追加・tracing::info! 使用 |

## 完了条件チェック

| 条件 | 状態 |
|---|---|
| Cargo.toml に tracing 追加 | 完了 |
| Cargo.toml に tracing-subscriber (env-filter) 追加 | 完了 |
| Cargo.toml に tower-http (trace) 追加 | 完了 |
| tracing_subscriber::fmt().with_env_filter(...).init() で初期化 | 完了 |
| Router に .layer(TraceLayer::new_for_http()) 付与 | 完了 |
| tracing::info! でリスニングアドレスを出力 | 完了 |
| 既存の日本語学習コメントを維持・拡充 | 完了 |

## 次ステップ

`/tumigi:direct-verify TASK-0002` を実行して以下を確認する:

1. `cargo run` でビルド・起動が成功する
2. `curl http://localhost:3000/` 時にコンソールへ `started processing request` / `finished processing request status=200` 相当のログが出る
3. `RUST_LOG=debug cargo run` でログレベルが debug に切り替わることを確認する
