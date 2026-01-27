import { AtUri } from "@atproto/syntax";
import { serve } from "@hono/node-server";
import { Hono } from "hono";

import { posts as oneyearagoPosts } from "oneyearago";
import { isFeedService, verifyAuth, type UserAuth } from "shared";

process.env.TZ = "UTC";

const startupTime = new Date().toISOString();
console.log(`App started at: ${startupTime}`);

// Rustサーバーへのプロキシ設定
const RUST_FEED_URL = process.env.RUST_FEED_URL || "http://localhost:3001";

const app = new Hono();

app.get("/", (c) => {
  console.log(`called route '/' ${startupTime}`);

  return c.text(
    "お試しでフィードを作っています https://github.com/girigiribauer/bluesky-feeds"
  );
});

app.get("/.well-known/did.json", (c) => {
  console.log(`called route '/.well-known/did.json' ${startupTime}`);

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
  console.log(
    `called route '/xrpc/app.bsky.feed.getFeedSkeleton' ${startupTime}`
  );

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

  // helloworld, todoappの場合はRustサーバーにプロキシ
  if (feedService === "helloworld" || feedService === "todoapp") {
    try {
      const queryString = new URLSearchParams(
        c.req.query() as Record<string, string>
      ).toString();
      const rustUrl = `${RUST_FEED_URL}/xrpc/app.bsky.feed.getFeedSkeleton?${queryString}`;

      console.log(`Proxying to Rust server: ${rustUrl}`);

      const headers: Record<string, string> = {};
      const auth = c.req.header("Authorization");
      if (auth) {
        headers["Authorization"] = auth;
      }

      const response = await fetch(rustUrl, {
        headers,
      });

      if (!response.ok) {
        const text = await response.text();
        console.error(`Rust server error details: ${text}`);
        throw new Error(`Rust server error: ${response.status} - ${text}`);
      }

      const data = await response.json();
      console.log(`Rust server response received`);
      return c.json(data);
    } catch (error) {
      console.error("Failed to proxy to Rust server:", error);
      throw error;
    }
  }

  // 他のフィードは従来通り処理
  let auth: UserAuth;
  switch (feedService) {

    case "oneyearago":
      auth = await verifyAuth(c.req);
      console.log(`did: ${auth.did}`);
      console.log("accessJwt:", auth.accessJwt);
      return c.json(await oneyearagoPosts(auth));
  }
});

serve({
  fetch: app.fetch,
  port: parseInt(process.env.PORT ?? "", 10) || 3000,
});
