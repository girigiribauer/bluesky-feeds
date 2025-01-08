# OneYearAgo feed

- ちょうど 1 年前の自分の投稿を表示する（パクリ）
  - 元ネタ https://bsky.app/profile/shigepon.net/feed/oneyearago
- https://docs.bsky.app/docs/api/app-bsky-feed-get-author-feed を使ってみるテスト
  - actor に自身の did を指定すれば自分の投稿が持ってこられる
  - searchPosts は since, until で期間指定できるものの、クエリ文字列なしの検索ができなかった
  - getAuthorFeed は cursor でそれ以前の投稿が遡れるものの、 since に相当する指定ができなかった

## 手元で動かす

root で実行

see http://localhost:3000/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:example:xxx/app.bsky.feed.generator/oneyearago
