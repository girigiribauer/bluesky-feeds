import { AtUri } from "@atproto/syntax";
import { serve } from "@hono/node-server";
import { Hono } from "hono";
import { posts as helloworldPosts } from "helloworld";
import { posts as todoappPosts } from "todoapp";
import { posts as oneyearagoPosts } from "oneyearago";
import { createBot } from "bot";
import { isFeedService, validateAuthHonoRequest } from "shared";

const app = new Hono();

app.get("/", (c) => {
  console.log("called route '/'");

  return c.text(
    "お試しでフィードを作っています https://github.com/girigiribauer/bluesky-toybox"
  );
});

app.get("/.well-known/did.json", (c) => {
  console.log("called route '/.well-known/did.json'");

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
  console.log("called route '/xrpc/app.bsky.feed.getFeedSkeleton'");

  const url = c.req.url;
  console.log(`url = ${url}`);
  const feed = c.req.query("feed");
  if (!feed) {
    console.error("Feed query param is missing");
    throw "Feed query param is missing";
  }
  console.log(`feed param = ${feed}`);

  const uri: AtUri = new AtUri(feed);
  const feedService = uri.rkey;
  if (!isFeedService(feedService)) {
    console.error("Feed service name is mismatch");
    throw "Feed service name is mismatch";
  }

  let did: string;
  switch (feedService) {
    case "helloworld":
      return c.json(await helloworldPosts());
    case "todoapp":
      did = await validateAuthHonoRequest(c.req);
      console.log(`did: ${did}`);
      return c.json(await todoappPosts(did));
    case "oneyearago":
      did = await validateAuthHonoRequest(c.req);
      console.log(`did: ${did}`);
      return c.json(await oneyearagoPosts(did));
  }
});

await createBot();

serve({
  fetch: app.fetch,
  port: parseInt(process.env.PORT ?? "", 10) || 3000,
});
