import { AtpAgent } from "@atproto/api";
import {
  isThreadViewPost,
  type PostView,
} from "@atproto/api/dist/client/types/app/bsky/feed/defs.js";
import type { Record } from "@atproto/api/dist/client/types/app/bsky/feed/post.js";
import type { Context } from "hono";
import { validateAuth, type FeedSkeletonResult } from "shared";

// TODO: テスト書く
const getTodo = async (did: string): Promise<string[]> => {
  const startTrigger = "TODO";
  const replyTrigger = "DONE";

  const agent = new AtpAgent({
    service: "https://public.api.bsky.app/",
  });

  const searchResponse = await agent.app.bsky.feed.searchPosts({
    q: startTrigger,
    author: did,
    limit: 100,
  });
  if (!searchResponse.success) {
    return [];
  }
  const posts = searchResponse.data.posts;

  const filterPost = async (post: PostView): Promise<boolean> => {
    const record = post.record as Record;

    if (!record.text.toLowerCase().startsWith(startTrigger.toLowerCase())) {
      return false;
    }

    if (post.replyCount === 0) {
      return true;
    }

    const threadResponse = await agent.app.bsky.feed.getPostThread({
      uri: post.uri,
    });
    if (!isThreadViewPost(threadResponse.data.thread)) {
      return false;
    }

    const replies = (threadResponse.data.thread.replies ?? []).filter((r) =>
      isThreadViewPost(r)
    );
    return !replies.find((r) => {
      const record = r.post.record as Record;
      return record.text.toLowerCase().startsWith(replyTrigger.toLowerCase());
    });
  };

  const filtered = (
    await Promise.all(
      posts.map(async (p) => ((await filterPost(p)) ? p : null))
    )
  ).filter((p) => p !== null);

  return filtered.map((a) => a.uri);
};

export const posts = async (c: Context): Promise<FeedSkeletonResult> => {
  const did = await validateAuth(c, "did:web:feeds.bsky.girigiribauer.com");
  const todoPosts = await getTodo(did);

  return {
    feed: todoPosts.map((uri) => ({
      post: uri,
    })),
  };
};
