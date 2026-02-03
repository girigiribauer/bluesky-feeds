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

## 公開・取り消しする (Rust)

```bash
# 公開・更新
cargo run --bin publish_feed <feed_id>
# 削除
cargo run --bin unpublish_feed <feed_id>
```

例: `cargo run --bin publish_feed oneyearago`
