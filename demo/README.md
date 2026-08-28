# デモ: 価格取得＆自動発注

裏側（ブラウザ拡張・マウス/キーボード非操作）の自動化と、表側（実際にマウス/キーボードを動かすGUI自動化）を1つのフローで対比するデモ。

## 構成

- `site/price.html` — 毎回ランダムな価格を表示するデモ株価ボード
- `site/order.html` — 数量・メモを入力して発注するデモフォーム（送信するとその場で受付番号を生成）
- `serve.cjs` — 上記2ページをローカル配信する簡易サーバー（依存パッケージなし、ローカルで試す場合用）
- `flow/price-and-order-demo.relay` — デモ用のRelayフロー

`site/` は GitHub Pages で公開済み: https://ganondorofu.github.io/desktop-flow-automation/
（`demo/site/` へのpushで `.github/workflows/deploy-demo-pages.yml` が自動デプロイする）
フロー内のURLはこのPages URLを直接指しているので、通常はローカルサーバーを起動する必要はない。

## 手順

1. Chromeに `browser-extension/` を読み込み（デベロッパーモード→パッケージ化されていない拡張機能を読み込む）。
2. Relayを起動し、`demo/flow/price-and-order-demo.relay` を開いて実行。

### ローカルで試す場合

Pages側を変更中などでローカル版を見たいときだけ:
```
node demo/serve.cjs
```
`http://localhost:8787` で配信される。この場合はフロー内のURLを `http://localhost:8787/...` に書き換えること。

## フローの流れ

**Phase 1（裏側・ヘッドレス）**
`launch_browser` → 価格ページを開いて `#price` を取得 → 正規表現で数値だけ抽出 →
発注ページへ遷移 → 数量・メモをJS経由でセット → 発注ボタンをクリック → 受付番号を取得

ここまでは拡張機能がDOMを直接操作するため、マウスカーソルもキーボードも一切動かない。

**Phase 2（表側・GUI）**
メモ帳を起動 → 取得した価格と受付番号を実際のキー入力でタイプ → マウスを実際に動かす →
完了メッセージを表示

同じ結果を得るのに、片方は画面を見ていても何も動いて見えず、もう片方は目に見えて動く — という対比がそのままデモになる。
