import dotenv from "dotenv";
import { AtpAgent } from "@atproto/api";

(async () => {
  dotenv.config();
  const handle = process.env.APP_HANDLE;
  const password = process.env.APP_PASSWORD;
  if (!handle || !password) {
    console.log("invalid handle or password");
    return;
  }

  const agent = new AtpAgent({ service: "https://bsky.social" });
  await agent.login({ identifier: handle, password });

  console.log(`login by ${handle}`);

  await agent.com.atproto.repo.putRecord({
    repo: agent.session?.did ?? "",
    collection: "app.bsky.feed.generator",
    rkey: "helloworld",
    record: {
      did: "did:web:helloworld.bsky.girigiribauer.com",
      displayName: "helloworld feed",
      description: "hello!",
      createdAt: new Date().toISOString(),
    },
  });

  console.log("published");
})();
