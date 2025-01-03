import { AtUri } from "@atproto/syntax";
import { serve } from "@hono/node-server";
import { Hono } from "hono";
import { posts as helloworldPosts } from "helloworld";
import { posts as todoappPosts } from "todoapp";
import { isFeedService } from "shared";

const app = new Hono();

app.get("/", (c) =>
  c.text(
    "お試しでフィードを作っています https://github.com/girigiribauer/bluesky-feeds"
  )
);

app.get("/.well-known/did.json", (c) => {
  return c.json({
    "@context": ["https://www.w3.org/ns/did/v1"],
    id: "did:web:feeds.bsky.girigiribauer.com",
    service: [
      {
        id: "#bsky_fg",
        type: "BskyFeedGenerator",
        serviceEndpoint: "https://feeds.bsky.girigiribauer.com",
      },
    ],
  });
});

app.get("/xrpc/app.bsky.feed.getFeedSkeleton", async (c) => {
  const feed = c.req.query("feed");
  if (!feed) {
    throw "Feed query param is missing";
  }

  const uri: AtUri = new AtUri(feed);
  const feedService = uri.rkey;
  if (!isFeedService(feedService)) {
    throw "Feed service name is wrong";
  }

  switch (feedService) {
    case "helloworld":
      return c.json(await helloworldPosts());
    case "todoapp":
      return c.json(await todoappPosts(c));
  }
});

serve({
  fetch: app.fetch,
  port: parseInt(process.env.PORT ?? "", 10) || 3000,
});
