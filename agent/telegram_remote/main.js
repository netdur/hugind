import { requireToken, getAdminUserId, getLoopSettings } from "./lib/config.js";
import { readOffset, writeOffset } from "./lib/offset_store.js";
import { clampTextForLog, splitTelegramText } from "./lib/text.js";
import { getUpdates, sendMessage } from "./lib/telegram_api.js";
import { extractTextMessages, nextOffsetFromUpdates } from "./lib/message_filter.js";
import { buildReplyForMessage } from "./lib/llm_command.js";

export default async function main(input) {
  const token = requireToken(input);
  const adminUserId = getAdminUserId(input);
  const { timeoutSeconds, maxUpdatesPerPoll } = getLoopSettings();

  let offset = readOffset();
  print(`[telegram_remote] live mode started timeout=${timeoutSeconds}s offset=${offset}`);

  while (true) {
    let updates;
    try {
      updates = await getUpdates(token, {
        offset,
        limit: maxUpdatesPerPoll,
        timeout: timeoutSeconds,
      });
    } catch (error) {
      print(`[telegram_remote] getUpdates_error="${clampTextForLog(String(error), 220)}"`);
      continue;
    }

    if (!Array.isArray(updates) || updates.length === 0) {
      continue;
    }

    offset = nextOffsetFromUpdates(updates, offset);
    writeOffset(offset);

    const messages = extractTextMessages(updates, adminUserId);
    print(`[telegram_remote] updates=${updates.length} actionable=${messages.length} offset=${offset}`);

    for (const message of messages) {
      if (!message.chatId) {
        continue;
      }

      let reply;
      try {
        reply = await buildReplyForMessage(message.text);
      } catch (error) {
        reply = `LLM error: ${String(error)}`;
      }

      print(
        `[telegram_remote] reply update_id=${message.updateId} chat_id=${message.chatId} ` +
          `in="${clampTextForLog(message.text)}" out="${clampTextForLog(reply)}"`
      );

      const chunks = splitTelegramText(reply);
      for (const chunk of chunks) {
        try {
          await sendMessage(token, message.chatId, chunk || "(empty response)");
        } catch (error) {
          print(
            `[telegram_remote] sendMessage_error chat_id=${message.chatId} ` +
              `error="${clampTextForLog(String(error), 220)}"`
          );
          break;
        }
      }
    }
  }
}
