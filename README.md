# bluesky-feeds

Bluesky のフィード置き場です

- **Helloworld フィード**
  - 死活監視用
  - せっかくなので2つ目以降のポストで `hello world` が入った文字を表示
  - 英語のみ（とはいえ hello world と打つだけなので、ある意味多言語）
- **Todo フィード**
  - `TODO` で始まるポストを表示して、 `DONE` の返信で消す
  - `from:me` 付きの searchPosts API を叩いて、DBレスで完結させる
  - https://docs.bsky.app/docs/api/app-bsky-feed-search-posts
- **OneYearAgo フィード**
  - n年前の同日の投稿を遡って表示する
  - プロフィールにタイムゾーンが書いてあればそれを優先し、日本語があれば日本時間を推定し、何もなければ世界標準時にする
  - https://docs.bsky.app/docs/api/app-bsky-actor-get-profile
- **Fake Bluesky フィード**
  - Bluesky とともに投稿した画像が実は青空じゃないものをピックアップして表示
  - 簡易的な画像処理で画像上部30%が青が多ければ青空、そうでないものは Fake とする
  - jetstream へ接続してみる用
  - https://docs.bsky.app/blog/jetstream

## ローカルでの実行

アプリパスワードは 1Password（`op://Automation/bluesky-feeds/app-password`）から `op run` で渡します。`op` にサインインした上で、以下のコマンドで起動します。

```bash
make dev
```

サーバーは `http://localhost:3000` で起動します。

## テストの実行

```bash
# 全テスト実行 (fmt, lint 含む)
make test

# 個別実行
make test-all         # テストのみ (Unit + Integration)
make test-unit        # ユニットテストのみ
make test-integration # 統合テストのみ
```

## 公開・取り消しする (Rust)

```bash
# 公開・更新
make publish FEED=<feed_id>
# 例: make publish FEED=helloworld

# 削除
make unpublish FEED=<feed_id>
```

## 開発者向けツール

### Fake Bluesky 画像判定チェッカー

特定の画像が Fake Bluesky の判定基準（青空成分の割合）に合致するかを確認するツールです。

```bash
# 使い方
make check-image IMAGE=<image_path_or_url>
# 例: make check-image IMAGE=./test.jpg
```

出力結果の `Blue Score` が `0.5` (50%) 未満であれば「Fake Bluesky」として採用 (ACCEPTED) されます。

## 使っている外部のモデルと部品

Silent Views フィードの consumer は、次の学習済みモデルと部品を使っています。モデルは ncnn 形式に変換して `crates/silentviews/consumer/models/` に置いています。ライセンスの全文と手を加えた点は [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES) にまとめています（consumer の Docker イメージにも同梱）。

| 名前 | 用途 | ライセンス |
| --- | --- | --- |
| [TinyCLIP](https://github.com/wkcn/TinyCLIP) (ViT-8M-16-Text-3M-YFCC15M) | 画像と言葉の照らし合わせ | MIT |
| [PP-OCRv3](https://github.com/opencv/opencv_zoo/tree/main/models/text_detection_ppocr)（opencv_zoo） | 文字探し | Apache-2.0 |
| [NanoDet](https://github.com/opencv/opencv_zoo/tree/main/models/object_detection_nanodet)（opencv_zoo） | 物探し | Apache-2.0 |
| [U-2-Net](https://github.com/xuebinqin/U-2-Net) u2netp（[rembg](https://github.com/danielgatis/rembg) の ONNX 書き出し） | 目立つ物の切り出し | Apache-2.0（ONNX 書き出しは MIT） |
| [ncnn](https://github.com/Tencent/ncnn) | モデルの実行 | BSD-3-Clause |
