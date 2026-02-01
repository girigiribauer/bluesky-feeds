# bluesky-feeds

Bluesky のフィード置き場です

- [helloworld](https://github.com/girigiribauer/bluesky-feeds/blob/main/packages/helloworld/README.md)
  - 最小構成でフィードを動かす
  - 固定ポストの表示なので API で引っ張って来てない例
- [todoapp](https://github.com/girigiribauer/bluesky-feeds/blob/main/packages/todoapp/README.md)
  - TODO 管理フィード
  - [searchPosts](https://docs.bsky.app/docs/api/app-bsky-feed-search-posts) の範囲で表示
- [oneyearago](https://github.com/girigiribauer/bluesky-feeds/blob/main/packages/oneyearago/README.md)
  - 1 年前フィード
  - [getAuthorFeed](https://docs.bsky.app/docs/api/app-bsky-feed-get-author-feed) API を活用

## 手元で動かす

全フィード共通

```
npm install
npm run dev
```

## 公開・取り消しする (Rust)

```bash
cd rust-feeds
# 公開・更新
cargo run --bin publish_feed <feed_id>
# 削除
cargo run --bin unpublish_feed <feed_id>
```

例: `cargo run --bin publish_feed oneyearago`
