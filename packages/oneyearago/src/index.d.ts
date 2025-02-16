import { type FeedSkeletonResult } from "shared";
type DateTimeRange = {
    since: Date;
    until: Date;
};
/**
 * 任意の日付を受け取り、1年前（閏年だった場合は2/28）の投稿を表示する
 * @param date 任意の日付
 * @returns
 */
export declare const getOneYearAgoRangeWithTZ: (date: Date) => DateTimeRange;
export declare const posts: (did: string) => Promise<FeedSkeletonResult>;
export {};
//# sourceMappingURL=index.d.ts.map