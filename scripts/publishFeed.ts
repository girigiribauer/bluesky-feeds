import dotenv from "dotenv";
import { AtpAgent } from "@atproto/api";

const Feeds = ["helloworld", "todoapp"] as const;
type FeedType = (typeof Feeds)[number];

type BlueskyConfig = {
  handle: string;
  password: string;
  feedName: FeedType;
};

const isFeedType = (name: string): name is FeedType => {
  return Feeds.includes(name as FeedType);
};

const publishFeed = async ({ handle, password, feedName }: BlueskyConfig) => {
  const agent = new AtpAgent({ service: "https://bsky.social" });
  await agent.login({ identifier: handle, password });

  console.log(`login by ${handle}`);

  await agent.com.atproto.repo.putRecord({
    repo: agent.session?.did ?? "",
    collection: "app.bsky.feed.generator",
    rkey: feedName,
    record: {
      did: "did:web:feeds.bsky.girigiribauer.com",
      displayName: "helloworld feed",
      description: "hello!",
      createdAt: new Date().toISOString(),
    },
  });

  console.log("published");
};

(async () => {
  dotenv.config();
  const handle = process.env.APP_HANDLE;
  const password = process.env.APP_PASSWORD;
  if (!handle || !password) {
    console.error("invalid handle or password");
    return;
  }

  if (process.argv.length <= 2 || !isFeedType(process.argv[2])) {
    console.error("FeedName argument is missing.");
    return;
  }
  const feedName = process.argv[2];

  publishFeed({ handle, password, feedName });
})();
