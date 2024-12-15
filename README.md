# bluesky-feed-helloworld

Bluesky のフィードを最小構成で作ってみます

- https://docs.bsky.app/docs/starter-templates/custom-feeds
- firehose 使わない、固定のポストを出すだけ
  - https://github.com/bluesky-social/feed-generator （参考）
  - https://qiita.com/rutan/items/a26652113935606c7855 （参考）
- Hono https://hono.dev/
- render https://render.com/

## 手元で動かす

```
npm install
npm run dev
```

## 公開・取り消しする

```
npm run publish
npm run unpublish
```

