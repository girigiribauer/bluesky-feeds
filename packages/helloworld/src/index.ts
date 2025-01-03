import type { FeedSkeletonResult } from "shared";

export const posts = async (): Promise<FeedSkeletonResult> => {
  return {
    feed: [
      {
        post: "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3ldcooerekc2y",
      },
    ],
  };
};
