import { AuthRequiredError, verifyJwt } from "@atproto/xrpc-server";
import { DidResolver, MemoryCache } from "@atproto/identity";
import type { HonoRequest } from "hono";
import { parseUrlNsid } from "@atproto/xrpc-server/dist/util.js";

export type FeedServiceType = (typeof FEED_SERVICES)[number];

export type FeedService = {
  service: FeedServiceType;
  displayName: string;
  description: string;
  avatar?: string;
};

export type FeedSkeletonResult = {
  cursor?: string;
  feed: {
    post: string;
  }[];
};

export type UserAuth = {
  did: string;
  accessJwt: string;
};

export const SERVICE_DID = "did:web:feeds.bsky.girigiribauer.com" as const;

export const FEED_SERVICES = ["helloworld", "todoapp", "oneyearago"] as const;

// TODO: もしかしたら各ワークスペース側に移した方が良さげ？
export const AVAILABLE_FEED_SERVICES: FeedService[] = [
  {
    service: "helloworld",
    displayName: "Helloworld feed",
    description: "Hello! Hello!",
  },
  {
    service: "todoapp",
    displayName: "TODO feed",
    description:
      "Only your posts starting with `TODO` are displayed. Replying with `DONE` will remove them.\n\n`TODO` と頭につけた自分の投稿だけが表示されます。 `DONE` と返信すると消えます。",
    avatar: "assets/todoapp.png",
  },
  {
    service: "oneyearago",
    displayName: "OneYearAgo feed",
    description:
      "Posts from exactly one year ago (±24 hours) are displayed.\n\nちょうど1年前の自分の投稿が表示されます（前後24時間）",
    avatar: "assets/oneyearago.png",
  },
];

export const isFeedService = (name: string): name is FeedServiceType => {
  return FEED_SERVICES.includes(name as FeedServiceType);
};

export const verifyAuth = async (
  honoRequest: HonoRequest
): Promise<UserAuth> => {
  console.log("called verifyAuth");
  const authorization = honoRequest.header("Authorization") ?? "";
  if (!authorization.startsWith("Bearer ")) {
    throw new AuthRequiredError();
  }

  const didCache = new MemoryCache();
  const didResolver = new DidResolver({
    plcUrl: "https://plc.directory",
    didCache,
  });

  const jwt = authorization.replace("Bearer ", "").trim();
  const nsid = parseUrlNsid(honoRequest.path);
  const parsed = await verifyJwt(
    jwt,
    SERVICE_DID,
    nsid,
    async (did: string) => {
      return didResolver.resolveAtprotoKey(did);
    }
  );

  console.log(`exp: ${parsed.exp}`);
  return {
    did: parsed.iss,
    accessJwt: jwt,
  };
};
