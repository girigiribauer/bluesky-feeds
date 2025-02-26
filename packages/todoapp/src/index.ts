import { AppBskyFeedSearchPosts, AtpAgent } from "@atproto/api";
import { decode, type JwtPayload } from "jsonwebtoken";
import {
  isThreadViewPost,
  type PostView,
} from "@atproto/api/dist/client/types/app/bsky/feed/defs.js";
import type { Record } from "@atproto/api/dist/client/types/app/bsky/feed/post.js";
import type { FeedSkeletonResult, UserAuth } from "shared";

const startTrigger = "TODO";
const replyTrigger = "DONE";

const filterPost = async (
  agent: AtpAgent,
  post: PostView
): Promise<boolean> => {
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

const getTodo = async (auth: UserAuth): Promise<string[]> => {
  const agent = new AtpAgent({
    service: "https://bsky.social",
    // fetch: (url, opts = {}) => {
    //   opts.headers = {
    //     ...opts.headers,
    //     Authorization: `Bearer ${auth.accessJwt}`,
    //   };
    //   console.log(opts);
    //   return fetch(url, opts);
    // },
  });

  // トークンのデコード
  const decoded = decode(auth.accessJwt);

  // 現在のUTC時刻を取得
  const currentTime = Math.floor(Date.now() / 1000); // 秒単位で取得

  let searchResponse: AppBskyFeedSearchPosts.Response;

  // トークンの有効期限（exp）をチェック
  if (
    (decoded as JwtPayload).payload.exp &&
    (decoded as JwtPayload).payload.exp > currentTime
  ) {
    console.log("JWTは有効期限内です。APIリクエストを送信します。");

    try {
      searchResponse = await agent.app.bsky.feed.searchPosts({
        q: startTrigger,
        author: auth.did,
        limit: 100,
      });
    } catch (error) {
      console.error("APIリクエスト中にエラーが発生しました:", error);
      throw "internal error";
    }
  } else {
    console.log("JWTが有効期限切れです。新しいトークンを取得してください。");
    throw "internal error";
  }

  if (!searchResponse.success) {
    return [];
  }
  const posts = searchResponse.data.posts;

  const filtered = (
    await Promise.all(
      posts.map(async (p) => ((await filterPost(agent, p)) ? p : null))
    )
  ).filter((p) => p !== null);

  return filtered.map((a) => a.uri);
};

export const posts = async (auth: UserAuth): Promise<FeedSkeletonResult> => {
  const todoPosts = await getTodo(auth);

  return {
    feed: todoPosts.map((uri) => ({
      post: uri,
    })),
  };
};
