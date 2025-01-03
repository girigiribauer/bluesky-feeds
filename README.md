# bluesky-feeds

Bluesky のフィード置き場です

- [helloworld](https://github.com/girigiribauer/bluesky-feeds/blob/main/packages/helloworld/README.md)
  - 最小構成でフィードを動かす
  - 固定ポストの表示なので API で引っ張ってこない
- [todoapp](https://github.com/girigiribauer/bluesky-feeds/blob/main/packages/todoapp/README.md)
  - TODO 管理フィード
  - [searchPosts](https://docs.bsky.app/docs/api/app-bsky-feed-search-posts) の範囲で表示

## 手元で動かす

全フィード共通

```
npm install
npm run dev
```

## 公開・取り消しする

```
npm run publish
npm run unpublish
```

## 構成

- Hono https://hono.dev/
- render https://render.com/
