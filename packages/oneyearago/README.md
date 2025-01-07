# OneYearAgo feed

- ちょうど 1 年前の自分の投稿を表示する（パクリ）
  - 元ネタ https://bsky.app/profile/shigepon.net/feed/oneyearago
- ~~https://docs.bsky.app/docs/api/app-bsky-feed-get-author-feed を使ってみるテスト~~ と思ったけど searchPosts の方が期間指定ができて使えたのでそっちに変更
  - ~~actor に自身の did を指定すれば自分の投稿が持ってこられる~~

## 手元で動かす

root で実行

see http://localhost:3000/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:example:xxx/app.bsky.feed.generator/oneyearago
