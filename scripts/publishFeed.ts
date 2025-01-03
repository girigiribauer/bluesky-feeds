import dotenv from "dotenv";
import fs from "fs/promises";
import { AtpAgent, BlobRef } from "@atproto/api";
import { AvailableFeedServices, isFeedService, type FeedService } from "shared";

type BlueskyConfig = {
  handle: string;
  password: string;
  feedService: FeedService;
};

const uploadAvatar = async (
  agent: AtpAgent,
  path: string
): Promise<BlobRef> => {
  const image = await fs.readFile(path);
  const blobResponse = await agent.com.atproto.repo.uploadBlob(image, {
    encoding: "image/png",
  });

  return blobResponse.data.blob;
};

const publishFeed = async ({
  handle,
  password,
  feedService,
}: BlueskyConfig) => {
  const agent = new AtpAgent({ service: "https://bsky.social" });
  await agent.login({ identifier: handle, password });

  console.log(`login by ${handle}`);

  const avatar: BlobRef | undefined = await (async () => {
    if (feedService.avatar) {
      return await uploadAvatar(agent, feedService.avatar).catch(
        () => undefined
      );
    }
    return;
  })();

  await agent.com.atproto.repo.putRecord({
    repo: agent.session?.did ?? "",
    collection: "app.bsky.feed.generator",
    rkey: feedService.service,
    record: {
      did: "did:web:feeds.bsky.girigiribauer.com",
      displayName: feedService.displayName,
      description: feedService.description,
      avatar,
      createdAt: new Date().toISOString(),
    },
  });

  console.log(`published ${feedService.service}`);
};

(async () => {
  dotenv.config();
  const handle = process.env.APP_HANDLE;
  const password = process.env.APP_PASSWORD;
  if (!handle || !password) {
    console.error("invalid handle or password");
    return;
  }

  if (process.argv.length <= 2 || !isFeedService(process.argv[2])) {
    console.error("FeedService argument is missing.");
    return;
  }

  const service = process.argv[2];
  const feedService = AvailableFeedServices.find((a) => a.service === service);
  if (!feedService) {
    console.error("FeedService definition is missing.");
    return;
  }

  publishFeed({ handle, password, feedService });
})();
