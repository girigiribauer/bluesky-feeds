import { AuthRequiredError, verifyJwt } from "@atproto/xrpc-server";
import { DidResolver, MemoryCache } from "@atproto/identity";
import type { Context } from "hono";
import { parseUrlNsid } from "@atproto/xrpc-server/dist/util.js";

export const FeedServices = ["helloworld", "todoapp"] as const;
export type FeedServiceType = (typeof FeedServices)[number];

export type FeedService = {
  service: FeedServiceType;
  displayName: string;
  description: string;
  avatar?: string;
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
      "Only your posts starting with `TODO` are displayed. Replying with `DONE` will remove them.\n`TODO` と頭につけた自分の投稿だけが表示されます。 `DONE` と返信すると消えます。",
    avatar: "assets/todoapp.png",
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

// TODO: リファクタ
export const validateAuth = async (
  c: Context,
  serviceDid: string
): Promise<string> => {
  const authorization = c.req.header("Authorization") ?? "";
  if (!authorization.startsWith("Bearer ")) {
    throw new AuthRequiredError();
  }

  const didCache = new MemoryCache();
  const didResolver = new DidResolver({
    plcUrl: "https://plc.directory",
    didCache,
  });

  const jwt = authorization.replace("Bearer ", "").trim();
  const nsid = parseUrlNsid(c.req.path);

  const parsed = await verifyJwt(jwt, serviceDid, nsid, async (did: string) => {
    return didResolver.resolveAtprotoKey(did);
  });

  return parsed.iss;
};
