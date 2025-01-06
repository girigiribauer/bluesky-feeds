import { expect, test } from "vitest";
import { posts } from "../src/index";

test("helloworld", async () => {
  const { feed } = await posts();

  expect(feed.length).toBe(1);
  expect(feed[0].post).toBe(
    "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3ldcooerekc2y"
  );
});
