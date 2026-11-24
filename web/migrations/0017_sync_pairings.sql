-- One message per device approval: the account's sync key, sealed on a device that has it with
-- a one-time code shown on a screen, waiting a few minutes for the new device to collect it
-- once. The code never reaches the server, so nothing here opens a budget.
CREATE TABLE sync_pairings_v2 (
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  id TEXT NOT NULL,
  wrapped TEXT NOT NULL,
  key_generation TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  -- Set when the new device takes the sealed key, which is blanked with it. The row stays until
  -- it expires, so the device that showed the code can tell "collected" from "ran out".
  collected INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(user_id,id)
);
CREATE INDEX sync_pairings_v2_expiry ON sync_pairings_v2(expires_at);
