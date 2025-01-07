import { AtpAgent } from "@atproto/api";
import type { Record } from "@atproto/api/dist/client/types/app/bsky/feed/post.js";
import { type FeedSkeletonResult } from "shared";

type TodayRange = {
  since: Date;
  until: Date;
};

type PostAccumulator = {
  cursor?: string;
  posts: string[];
};

export const getOneYearAgoRangeWithTZ = (date: Date): TodayRange => {
  // TODO: とりあえず正しい実装はあとでやる、今はざっくり1年前の1日分を出す
  // TODO: テスト書く
  const since = new Date(date.valueOf() - 365 * 24 * 60 * 60 * 1000);
  const until = new Date(since.valueOf() + 24 * 60 * 60 * 1000);

  return {
    since,
    until,
  };
};

const getOneDayPosts = async (
  agent: AtpAgent,
  did: string,
  range: TodayRange,
  acc: PostAccumulator = { posts: [] }
): Promise<string[]> => {
  const cursor = acc.cursor ? acc.cursor : range.since.toISOString();
  const searchResponse = await agent.app.bsky.feed.getAuthorFeed({
    actor: did,
    cursor,
    limit: 100,
  });

  if (!searchResponse.success) {
    console.warn("searchPosts failure");
    return [];
  }

  const posts = searchResponse.data.feed
    .filter((a) => {
      if (a.post.author.did !== did) {
        return false;
      }

      const record = a.post.record as Record;
      return new Date(record.createdAt).valueOf() <= range.until.valueOf();
    })
    .map((a) => a.post.uri);

  acc.posts = acc.posts.concat(posts);
  if (
    searchResponse.data.cursor &&
    range.until.valueOf() <= new Date(searchResponse.data.cursor).valueOf()
  ) {
    acc.cursor = searchResponse.data.cursor;
    await new Promise((resolve) => {
      setTimeout(() => resolve, 1000);
    });
    return await getOneDayPosts(agent, did, range, acc);
  }

  return acc.posts;
};

export const posts = async (did: string): Promise<FeedSkeletonResult> => {
  const agent = new AtpAgent({
    service: "https://public.api.bsky.app/",
  });

  const range = getOneYearAgoRangeWithTZ(new Date());
  const posts = await getOneDayPosts(agent, did, range);

  return {
    feed: posts.map((post) => ({ post })),
  };
};
