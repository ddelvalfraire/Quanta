import type { Channel } from "phoenix";

export interface PresenceUser {
  userId: string;
  joinedAt: number;
}

export function setupPresence(
  channel: Channel,
  onUpdate: (users: PresenceUser[]) => void
): () => void {
  const users = new Map<string, PresenceUser>();

  const refState = channel.on("presence_state", (state: unknown) => {
    const entries = state as Record<
      string,
      { metas: Array<{ joined_at: number }> }
    >;
    users.clear();
    for (const [userId, data] of Object.entries(entries)) {
      users.set(userId, {
        userId,
        joinedAt: data.metas[0]?.joined_at ?? 0,
      });
    }
    onUpdate(Array.from(users.values()));
  });

  const refDiff = channel.on("presence_diff", (diff: unknown) => {
    const { joins, leaves } = diff as {
      joins: Record<string, { metas: Array<{ joined_at: number }> }>;
      leaves: Record<string, unknown>;
    };
    for (const [userId, data] of Object.entries(joins)) {
      users.set(userId, {
        userId,
        joinedAt: data.metas[0]?.joined_at ?? 0,
      });
    }
    for (const userId of Object.keys(leaves)) {
      users.delete(userId);
    }
    onUpdate(Array.from(users.values()));
  });

  return () => {
    channel.off("presence_state", refState);
    channel.off("presence_diff", refDiff);
  };
}
