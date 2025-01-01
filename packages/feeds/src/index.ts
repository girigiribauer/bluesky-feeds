import { serve } from "@hono/node-server";
import { Hono } from "hono";

const app = new Hono();

app.get("/", (c) =>
  c.text(
    "お試しでフィードを作っています https://github.com/girigiribauer/bluesky-feeds"
  )
);

app.get("/xrpc/app.bsky.feed.getFeedSkeleton", (c) =>
  c.json({
    feed: [
      {
        post: "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3ldcooerekc2y",
      },
    ],
  })
);

serve({
  fetch: app.fetch,
  port: parseInt(process.env.PORT ?? "", 10) || 3000,
});
