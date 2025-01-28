import { Bot } from "@skyware/bot";
import dotenv from "dotenv";
import { scheduleJob } from "node-schedule";

export const createBot = async (): Promise<void> => {
  dotenv.config({ path: "../../.env" });
  const handle = process.env.BOT_HANDLE;
  const password = process.env.BOT_PASSWORD;
  if (!handle || !password) {
    console.error("invalid handle or password");
    return;
  }

  const bot = new Bot();
  await bot.login({
    identifier: handle,
    password: password,
  });

  bot.on("mention", async (reply) => {
    console.log(`mention: from ${reply.uri}`);
    await reply.like();
    await reply.reply({
      text: "ありがとうございます！自動運用のためお返事返せません！",
    });
  });

  bot.on("reply", async (reply) => {
    console.log(`reply: from ${reply.uri}`);
    await reply.like();
    await reply.reply({
      text: "ありがとうございます！自動運用のためお返事返せません！",
    });
  });

  scheduleJob("42 * * * *", async (fireDate: Date) => {
    const time = new Intl.DateTimeFormat("ja-JP", {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
      timeZone: "Asia/Tokyo",
    }).format(fireDate);

    const text = `毎時42分に投稿する自動運用テストです！ (${time})`;
    console.log(`scheduled: ${text}`);
    await bot.post({ text });
  });
};
