export const FeedServices = ["helloworld", "todoapp"] as const;
export type FeedServiceType = (typeof FeedServices)[number];

export type FeedService = {
  service: FeedServiceType;
  displayName: string;
  description: string;
};

export const AvailableFeedServices: FeedService[] = [
  {
    service: "helloworld",
    displayName: "Helloworld feed",
    description: "Hello! Hello!",
  },
  {
    service: "todoapp",
    displayName: "TODO feed",
    description:
      "TODO と頭につけた自分の投稿だけが表示されます\nDONE と返信すると消えます\nrender.com を無料プランでテストしてるので、15分だれも利用がないとサーバーが止まっちゃうみたいです。なんとかするので再読み込みなどしてみてください :pray:",
  },
];

export const isFeedService = (name: string): name is FeedServiceType => {
  return FeedServices.includes(name as FeedServiceType);
};

export type FeedSkeletonResult = {
  cursor?: string;
  feed: {
    post: string;
  }[];
};
