# bluesky-feeds

Bluesky のフィード置き場です

- [helloworld](https://github.com/girigiribauer/bluesky-feeds/tree/main/packages/helloworld/README.md)
  - 最小構成でフィードを動かす
  - see http://localhost:3000/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:example:xxx/app.bsky.feed.generator/helloworld

## 公開・取り消しする

```
npm run publish
npm run unpublish
```

## 構成

- Hono https://hono.dev/
- render https://render.com/
