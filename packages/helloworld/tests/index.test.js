import { describe, expect, it } from "vitest";
import { posts } from "../src/index";
describe("posts", () => {
    it("固定ポストが一致する", async () => {
        const { feed } = await posts();
        expect(feed.length).toBe(1);
        expect(feed[0].post).toBe("at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3ldcooerekc2y");
    });
});
