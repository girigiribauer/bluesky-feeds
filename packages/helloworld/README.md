# helloworld feed

Bluesky のフィードを最小構成で作ってみます

- https://docs.bsky.app/docs/starter-templates/custom-feeds
- firehose 使わない、固定のポストを出すだけ
  - https://github.com/bluesky-social/feed-generator （参考）
  - https://qiita.com/rutan/items/a26652113935606c7855 （参考）

## 手元で動かす

```
npm install
npm run dev
```

see http://localhost:3000/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:example:xxx/app.bsky.feed.generator/helloworld
