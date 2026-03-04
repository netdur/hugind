export function extractTextMessages(updates, adminUserId = "") {
  const out = [];
  for (const update of updates) {
    const msg = update?.message;
    const text = msg?.text;
    if (!text) {
      continue;
    }

    const fromId = String(msg?.from?.id ?? "");
    if (adminUserId && fromId !== adminUserId) {
      continue;
    }

    out.push({
      updateId: update.update_id,
      chatId: msg.chat?.id,
      text,
    });
  }
  return out;
}

export function nextOffsetFromUpdates(updates, currentOffset) {
  let maxUpdateId = currentOffset > 0 ? currentOffset - 1 : -1;
  for (const update of updates) {
    const id = update?.update_id;
    if (typeof id === "number" && id > maxUpdateId) {
      maxUpdateId = id;
    }
  }
  return maxUpdateId >= 0 ? maxUpdateId + 1 : currentOffset;
}
