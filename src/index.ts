import { serve } from "@hono/node-server";
import { Hono } from "hono";

const app = new Hono();

app.get("/.well-known/did.json", (c) =>
  c.json({
    "@context": ["https://www.w3.org/ns/did/v1"],
    id: "did:web:helloworld.bsky.girigiribauer.com",
    service: [
      {
        id: "#bsky_fg",
        type: "BskyFeedGenerator",
        serviceEndpoint: "https://helloworld.bsky.girigiribauer.com",
      },
    ],
  })
);

app.get("/xrpc/app.bsky.feed.getFeedSkeleton", (c) =>
  c.json({
    feed: [
      {
        post: "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3ldcooerekc2y",
      },
      {
        post: "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3ldcooerekc2y",
      },
    ],
  })
);

serve({
  fetch: app.fetch,
  port: parseInt(process.env.PORT ?? "", 10) ?? 3000,
});
