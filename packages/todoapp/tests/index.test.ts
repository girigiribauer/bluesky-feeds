import { setupServer, SetupServerApi } from "msw/node";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import { posts } from "../src/index";
import { http, HttpResponse } from "msw";

type handlerResponseType =
  | "zero"
  | "oneTodoSuccess"
  | "oneTodoFailure"
  | "oneDoneSuccess"
  | "oneDoneFailure";
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

const threadTemplate = {
  thread: {
    $type: "app.bsky.feed.defs#threadViewPost",
    post: postTemplate,
    replies: [
      {
        $type: "app.bsky.feed.defs#threadViewPost",
        post: postTemplate,
        replies: [],
      },
    ],
  },
};

const handlers = [
  http.get("https://public.api.bsky.app/xrpc/app.bsky.feed.searchPosts", () => {
    switch (handlerResponse) {
      case "zero":
        return HttpResponse.json({ posts: [] });
      case "oneTodoSuccess":
        return HttpResponse.json({
          posts: [
            Object.assign({}, postTemplate, {
              record: {
                text: "TODO が先頭にある投稿",
              },
            }),
          ],
        });
      case "oneTodoFailure":
        return HttpResponse.json({
          posts: [
            Object.assign({}, postTemplate, {
              record: {
                text: "テスト TODO が途中にある投稿",
              },
            }),
          ],
        });
      case "oneDoneSuccess":
      case "oneDoneFailure":
        return HttpResponse.json({
          posts: [
            Object.assign({}, postTemplate, {
              record: {
                text: "TODO test",
              },
              replyCount: 1,
            }),
          ],
        });
      default:
        throw new Error("handlerResponse is mismatch");
    }
  }),

  http.get(
    "https://public.api.bsky.app/xrpc/app.bsky.feed.getPostThread",
    () => {
      switch (handlerResponse) {
        case "oneDoneSuccess":
          return HttpResponse.json(
            Object.assign({}, threadTemplate, {
              thread: {
                ...threadTemplate.thread,
                replies: [
                  {
                    ...threadTemplate.thread.replies[0],
                    post: {
                      ...threadTemplate.thread.replies[0].post,
                      record: {
                        ...threadTemplate.thread.replies[0].post.record,
                        text: "DONE",
                      },
                    },
                  },
                ],
              },
            })
          );
        case "oneDoneFailure":
          return HttpResponse.json(
            Object.assign({}, threadTemplate, {
              thread: {
                ...threadTemplate.thread,
                replies: [
                  {
                    ...threadTemplate.thread.replies[0],
                    post: {
                      ...threadTemplate.thread.replies[0].post,
                      record: {
                        ...threadTemplate.thread.replies[0].post.record,
                        text: "foo DONE bar",
                      },
                    },
                  },
                ],
              },
            })
          );
        default:
          return HttpResponse.json(threadTemplate);
      }
    }
  ),
];

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

  it("先頭 TODO 投稿1件の時", async () => {
    handlerResponse = "oneTodoSuccess";
    const { feed } = await posts("did:example:xxx");

    expect(feed.length).toBe(1);
  });

  it("途中 TODO 投稿1件の時", async () => {
    handlerResponse = "oneTodoFailure";
    const { feed } = await posts("did:example:xxx");

    expect(feed.length).toBe(0);
  });

  it("先頭 DONE 返信1件の時", async () => {
    handlerResponse = "oneDoneSuccess";
    const { feed } = await posts("did:example:xxx");

    expect(feed.length).toBe(0);
  });

  it("途中 DONE 返信1件の時", async () => {
    handlerResponse = "oneDoneFailure";
    const { feed } = await posts("did:example:xxx");

    expect(feed.length).toBe(1);
  });
});
