import dotenv from "dotenv";
import { AtpAgent } from "@atproto/api";
import { AVAILABLE_FEED_SERVICES, isFeedService } from "shared";

(async () => {
  dotenv.config();
  const handle = process.env.APP_HANDLE;
  const password = process.env.APP_PASSWORD;
  if (!handle || !password) {
    console.log("invalid handle or password");
    return;
  }

  if (process.argv.length <= 2 || !isFeedService(process.argv[2])) {
    console.error("FeedService argument is missing.");
    return;
  }

  const service = process.argv[2];
  const feedService = AVAILABLE_FEED_SERVICES.find(
    (a) => a.service === service
  );
  if (!feedService) {
    console.error("FeedService definition is missing.");
    return;
  }

  const agent = new AtpAgent({ service: "https://bsky.social" });
  await agent.login({ identifier: handle, password });

  console.log(`login! ${handle}`);

  await agent.com.atproto.repo.deleteRecord({
    repo: agent.session?.did ?? "",
    collection: "app.bsky.feed.generator",
    rkey: feedService.service,
  });

  console.log("unpublish");
})();
