# Relay(仮称)

Power Automate Desktop(PAD)を代替しうる、Windows向けデスクトップ自動化アプリ。

## コンセプト

- 知識がなくても組める(簡易モード)/知識があれば高度に組める(上級モード)を両立する
- 画像認識は完全一致だけでなく、位置・サイズ・ノイズのブレに強いAI/特徴点ベースの認識に対応する
- フローはGUIでもコードでも編集できる(ノーコード⇄コードの二層構造)
- Office/クラウド連携などMicrosoft資産との統合はスコープ外。デスクトップワークフローに集中する

## ドキュメント

- [要件まとめ](docs/requirements.md) — 機能要件・非機能要件と優先度
- [技術構成](docs/architecture.md) — スタック選定と理由、データモデル、レイヤー構成
- [ロードマップ](docs/roadmap.md) — 実装フェーズと順序

## 技術スタック(概要)

- フロントエンド: Tauri + React + React Flow(ノードエディタ)
- バックエンド: Rust(cargo workspace: `flow-schema` / `automation` / `engine` / `vision`)
- UI操作: `windows-rs` / `uiautomation-rs`(UI Automation API)、Win32 API(SendInput等)
- 画像認識: `image` / `imageproc`(純正Rust、OpenCV不要)によるテンプレートマッチング。ORB特徴点・AI類似検索は未着手(docs/roadmap.md参照)

詳細は [docs/architecture.md](docs/architecture.md) を参照。

## 開発メモ

**動作確認は基本これで足りる:**

```
cargo test --workspace --lib   # Rust全クレートのテスト。Bashでもそのまま動く
npm run build                  # フロントのビルド確認。Bashでもそのまま動く
```

`cargo test`は副作用なし(実マウス操作・実クリックなし)で安全に何度でも回せる。実画面キャプチャなど実OSに触れる検証は`#[ignore]`付きテストにしてあるので普段のテストには含まれない(例: `cargo test -p automation -- --ignored`)。

**`npm run test:rust` / `npm run verify` / `npm run tauri dev` はPowerShell専用:**

これらはnpm(Node)がcargoを子プロセスとして起動する。Git Bash上のPATHはPOSIX形式(`:`区切り)なのでWindowsネイティブな子プロセス起動がcargoを見つけられず失敗する。**PowerShellから**、かつ毎回セッション先頭で

```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin"
```

を実行してから使うこと(このハーネスのPowerShellプロセスは`.cargo/bin`がユーザーPATHに登録済みでも自動では拾わない)。
