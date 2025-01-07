# Todo feed

- 頭に `TODO` って書いてある自分の投稿のみを表示する
- 返信に `DONE` って書いてあったら消える
- https://docs.bsky.app/docs/api/app-bsky-feed-search-posts を使ってみるテスト
  - did に自身を指定すれば自分の投稿が持ってこられる

## 手元で動かす

root で実行

see http://localhost:3000/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:example:xxx/app.bsky.feed.generator/todoapp
