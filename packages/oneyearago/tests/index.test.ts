import { setupServer, SetupServerApi } from "msw/node";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import { getOneYearAgoRangeWithTZ, posts } from "../src/index";
import { http, HttpResponse } from "msw";

type handlerResponseType = "zero" | "one";
let handlerResponse: handlerResponseType;

const postTemplate = {
  uri: "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/xxxxxxxxxxxxx",
  cid: "bafyreigyqux6joat7eyrzedagxlf46q6yk3lldpioycubhfd62kpupb2dm",
  author: {
    did: "did:plc:tsvcmd72oxp47wtixs4qllyi",
    handle: "girigiribauer.com",
    displayName: "girigiribauer",
    avatar:
      "https://cdn.bsky.app/img/avatar/plain/did:plc:tsvcmd72oxp47wtixs4qllyi/bafkreib4aejfxx4tmlmqif6nbnkfq5vymxlvavw7u4dgk2d4ci5bhmxqpa@jpeg",
    associated: { chat: { allowIncoming: "all" } },
    labels: [],
    createdAt: "2023-04-13T03:50:20.194Z",
  },
  record: {
    $type: "app.bsky.feed.post",
    createdAt: "2025-01-03T13:12:46.119Z",
    langs: ["ja"],
    text: "xxx",
  },
  replyCount: 0,
  repostCount: 0,
  likeCount: 1,
  quoteCount: 0,
  indexedAt: "2025-01-03T13:12:47.153Z",
  labels: [],
};

const handlers = [
  http.get(
    "https://public.api.bsky.app/xrpc/app.bsky.feed.getAuthorFeed",
    () => {
      switch (handlerResponse) {
        case "zero":
          return HttpResponse.json({ feed: [] });
        case "one":
          return HttpResponse.json({ feed: [] });
        default:
          throw new Error("handlerResponse is mismatch");
      }
    }
  ),
];

describe("getOneYearAgoRangeWithTZ", () => {
  it("UTC 1/2 00:00:00 (JST 1/2 09:00:00) だと UTC 1/1 15:00:00 (JST 1/2 00:00:00) から UTC 1/2 15:00:00 (JST 1/3 00:00:00) を返す", () => {
    const result = getOneYearAgoRangeWithTZ(
      new Date("2025-01-02T00:00:00.000Z")
    );
    expect(result.since).toEqual(new Date("2024-01-01T15:00:00.000Z"));
    expect(result.until).toEqual(new Date("2024-01-02T15:00:00.000Z"));
  });

  it("UTC 1/1 15:00:00 (JST 1/2 00:00:00) だと UTC 1/1 15:00:00 (JST 1/2 00:00:00) から UTC 1/2 15:00:00 (JST 1/3 00:00:00) を返す", () => {
    const result = getOneYearAgoRangeWithTZ(
      new Date("2025-01-01T15:00:00.000Z")
    );
    expect(result.since).toEqual(new Date("2024-01-01T15:00:00.000Z"));
    expect(result.until).toEqual(new Date("2024-01-02T15:00:00.000Z"));
  });

  it("UTC 1/2 14:59:59 (JST 1/2 23:59:59) だと UTC 1/1 15:00:00 (JST 1/2 00:00:00) から UTC 1/2 15:00:00 (JST 1/3 00:00:00) を返す", () => {
    const result = getOneYearAgoRangeWithTZ(
      new Date("2025-01-02T14:59:59.000Z")
    );
    expect(result.since).toEqual(new Date("2024-01-01T15:00:00.000Z"));
    expect(result.until).toEqual(new Date("2024-01-02T15:00:00.000Z"));
  });
});

describe("posts", () => {
  let server: SetupServerApi;

  beforeAll(async () => {
    server = setupServer(...handlers);
    server.listen({ onUnhandledRequest: "error" });
  });

  afterAll(() => server.close());

  afterEach(() => server.resetHandlers());

  it("空の時", async () => {
    handlerResponse = "zero";
    const { feed } = await posts("did:example:xxx");

    expect(feed.length).toBe(0);
  });

  // TODO: 他のテスト書く
});

// TODO: getOneYearAgoRangeWithTZ のテスト書く
