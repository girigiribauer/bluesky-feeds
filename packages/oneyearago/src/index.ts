import { AtpAgent } from "@atproto/api";
import type { Record } from "@atproto/api/dist/client/types/app/bsky/feed/post.js";
import { type FeedSkeletonResult, type UserAuth } from "shared";

type DateTimeRange = {
  since: Date;
  until: Date;
};

type PostAccumulator = {
  cursor?: string;
  posts: string[];
};

/**
 * 任意の日付を受け取り、1年前（閏年だった場合は2/28）の投稿を表示する
 * @param date 任意の日付
 * @returns
 */
export const getOneYearAgoRangeWithTZ = (date: Date): DateTimeRange => {
  // const isLeapYear = (year: number) => {
  //   return (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
  // };

  // const utcZeroTime = ((date: Date) => {
  //   if (isLeapYear(year) && month == 2 && dayOfMonth == 29) {
  //     return new Date(`${year - 1}-02-28T00:00:00.000Z`);
  //   }
  //   const monthString = `${month}`.padStart(2, "0");
  //   const dayOfMonthString = `${dayOfMonth}`.padStart(2, "0");

  //   return new Date(
  //     `${year - 1}-${monthString}-${dayOfMonthString}T00:00:00.000Z`
  //   );
  // })(date);

  // const offset = date.getTimezoneOffset() * 60 * 1000;
  const year = String(date.getFullYear() - 1);
  const month = `${date.getMonth() + 1}`.padStart(2, "0");
  const dayOfMonth = `${date.getDate()}`.padStart(2, "0");
  const timeString = date.toISOString().split("T")[1];
  const oneYearAgo = new Date(`${year}-${month}-${dayOfMonth}T${timeString}`);
  const since = new Date(oneYearAgo.valueOf() - 24 * 60 * 60 * 1000);
  const until = new Date(oneYearAgo.valueOf());

  return {
    since,
    until,
  };
};

const getOneDayPosts = async (
  agent: AtpAgent,
  auth: UserAuth,
  range: DateTimeRange,
  acc: PostAccumulator = { posts: [] }
): Promise<string[]> => {
  const cursor = acc.cursor ? acc.cursor : range.until.toISOString();
  const searchResponse = await agent.app.bsky.feed.getAuthorFeed({
    actor: auth.did,
    cursor,
    limit: 100,
  });

  if (!searchResponse.success) {
    console.warn("searchPosts failure");
    return [];
  }

  const posts = searchResponse.data.feed.filter((a) => {
    if (a.post.author.did !== auth.did) {
      return false;
    }

    const record = a.post.record as Record;
    return range.since.valueOf() <= new Date(record.createdAt).valueOf();
  });

  acc.posts = acc.posts.concat(posts.map((a) => a.post.uri));
  if (
    searchResponse.data.cursor &&
    range.until.valueOf() <= new Date(searchResponse.data.cursor).valueOf()
  ) {
    acc.cursor = searchResponse.data.cursor;
    await new Promise((resolve) => {
      setTimeout(() => resolve, 1000);
    });
    return await getOneDayPosts(agent, auth, range, acc);
  }

  return acc.posts;
};

export const posts = async (auth: UserAuth): Promise<FeedSkeletonResult> => {
  const agent = new AtpAgent({
    service: "https://public.api.bsky.app",
  });

  const range = getOneYearAgoRangeWithTZ(new Date());
  const posts = await getOneDayPosts(agent, auth, range);

  return {
    feed: posts.map((post) => ({ post })),
  };
};
