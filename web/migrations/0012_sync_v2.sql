-- Dormant until the compatibility server explicitly activates an account.
CREATE TABLE sync_accounts_v2 (
  user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  protocol INTEGER NOT NULL DEFAULT 1 CHECK(protocol IN (1,2)),
  key_generation TEXT,
  salt TEXT,
  key_check TEXT,
  catalog_epoch TEXT NOT NULL,
  catalog_sequence TEXT NOT NULL DEFAULT '0',
  min_cursor TEXT NOT NULL DEFAULT '0',
  applied_by TEXT,
  legacy_cleanup INTEGER NOT NULL DEFAULT 0 CHECK(legacy_cleanup IN (0,1)),
  updated_at TEXT NOT NULL
);
CREATE TABLE sync_installations_v2 (
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  installation_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','retired')),
  last_sequence TEXT NOT NULL DEFAULT '0',
  last_digest TEXT,
  receipt TEXT,
  created_at TEXT NOT NULL,
  PRIMARY KEY(user_id,installation_id)
);
-- This ownership ledger deliberately survives the account: cleanup cannot disappear
-- with an ON DELETE CASCADE. It contains only opaque identifiers/checksums.
CREATE TABLE sync_objects_v2 (
  object_key TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  installation_id TEXT,
  sequence TEXT,
  digest TEXT,
  sha256 TEXT,
  size INTEGER NOT NULL DEFAULT 0,
  state TEXT NOT NULL CHECK(state IN ('uploading','ready','adopted','retired')),
  created_at TEXT NOT NULL,
  expires_at TEXT
);
CREATE INDEX sync_objects_v2_owner ON sync_objects_v2(user_id,state);
CREATE TABLE sync_heads_v2 (
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  budget_id TEXT NOT NULL,
  incarnation TEXT NOT NULL,
  version TEXT NOT NULL,
  key_generation TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('active','trashed','purged')),
  object_key TEXT REFERENCES sync_objects_v2(object_key),
  payload TEXT,
  size INTEGER NOT NULL DEFAULT 0,
  updated_sequence TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  retained_until TEXT,
  device TEXT NOT NULL DEFAULT '',
  applied_by TEXT NOT NULL,
  PRIMARY KEY(user_id,budget_id)
);
CREATE INDEX sync_heads_v2_objects ON sync_heads_v2(object_key);
CREATE INDEX sync_heads_v2_trash ON sync_heads_v2(user_id,state,budget_id);
CREATE INDEX sync_heads_v2_expiry ON sync_heads_v2(retained_until) WHERE state='trashed';
CREATE TABLE sync_staging_v2 (
  user_id TEXT NOT NULL,
  installation_id TEXT NOT NULL,
  sequence TEXT NOT NULL,
  digest TEXT NOT NULL,
  manifest TEXT NOT NULL,
  object_key TEXT REFERENCES sync_objects_v2(object_key),
  expires_at TEXT NOT NULL,
  claim TEXT,
  decision TEXT,
  PRIMARY KEY(user_id,installation_id),
  FOREIGN KEY(user_id,installation_id) REFERENCES sync_installations_v2(user_id,installation_id) ON DELETE CASCADE
);
CREATE INDEX sync_staging_v2_expiry ON sync_staging_v2(expires_at);
CREATE INDEX sync_staging_v2_objects ON sync_staging_v2(object_key);
CREATE TABLE sync_events_v2 (
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  sequence TEXT NOT NULL,
  epoch TEXT NOT NULL,
  event TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY(user_id,sequence)
);
CREATE INDEX sync_events_v2_retention ON sync_events_v2(created_at);
CREATE INDEX sync_events_v2_cursor ON sync_events_v2(user_id,epoch,CAST(sequence AS INTEGER));
CREATE TABLE sync_maintenance_v2(name TEXT PRIMARY KEY,value TEXT NOT NULL);

-- A pre-upgrade Worker can finish an ambiguous v1 publication after cutover and
-- run its old discardObject() query (which knows only sync_budgets). Suppress its
-- INSERT ... RETURNING so it never receives an adopted v2 key to blindly delete.
CREATE TRIGGER sync_v2_protect_adopted_garbage BEFORE INSERT ON sync_object_garbage
WHEN EXISTS(SELECT 1 FROM sync_heads_v2 WHERE object_key=NEW.object_key)
BEGIN SELECT RAISE(IGNORE); END;

-- Both schemas can reference the same immutable legacy object during cutover.
-- These triggers enqueue; the worker checks ALL reference types before deletion.
CREATE TRIGGER sync_v2_head_deleted AFTER DELETE ON sync_heads_v2 WHEN OLD.object_key IS NOT NULL BEGIN
  INSERT OR IGNORE INTO sync_object_garbage(object_key) VALUES(OLD.object_key);
END;
CREATE TRIGGER sync_v2_head_replaced AFTER UPDATE OF object_key ON sync_heads_v2
WHEN OLD.object_key IS NOT NULL AND OLD.object_key IS NOT NEW.object_key BEGIN
  INSERT OR IGNORE INTO sync_object_garbage(object_key) VALUES(OLD.object_key);
END;
CREATE TRIGGER sync_v2_staging_deleted AFTER DELETE ON sync_staging_v2 WHEN OLD.object_key IS NOT NULL BEGIN
  UPDATE sync_objects_v2 SET state='retired' WHERE object_key=OLD.object_key AND state<>'adopted';
  INSERT OR IGNORE INTO sync_object_garbage(object_key) VALUES(OLD.object_key);
END;

-- Fence requests that entered an old Worker before activation. The explicit
-- account-deletion guard (or a deleted parent) permits the controlled cascade.
CREATE TRIGGER sync_v2_fence_legacy_budget_insert BEFORE INSERT ON sync_budgets
WHEN EXISTS(SELECT 1 FROM sync_accounts_v2 WHERE user_id=NEW.user_id AND protocol=2 AND legacy_cleanup=0)
  AND NOT EXISTS(SELECT 1 FROM account_deletions WHERE user_id=NEW.user_id)
BEGIN SELECT RAISE(ABORT,'sync_upgrade_required'); END;
CREATE TRIGGER sync_v2_fence_legacy_budget_update BEFORE UPDATE ON sync_budgets
WHEN EXISTS(SELECT 1 FROM sync_accounts_v2 WHERE user_id=OLD.user_id AND protocol=2 AND legacy_cleanup=0)
  AND NOT EXISTS(SELECT 1 FROM account_deletions WHERE user_id=OLD.user_id)
BEGIN SELECT RAISE(ABORT,'sync_upgrade_required'); END;
CREATE TRIGGER sync_v2_fence_legacy_budget_delete BEFORE DELETE ON sync_budgets
WHEN EXISTS(SELECT 1 FROM sync_accounts_v2 WHERE user_id=OLD.user_id AND protocol=2 AND legacy_cleanup=0)
  AND EXISTS(SELECT 1 FROM users WHERE id=OLD.user_id)
  AND NOT EXISTS(SELECT 1 FROM account_deletions WHERE user_id=OLD.user_id)
BEGIN SELECT RAISE(ABORT,'sync_upgrade_required'); END;
CREATE TRIGGER sync_v2_fence_legacy_key_insert BEFORE INSERT ON sync_keys
WHEN EXISTS(SELECT 1 FROM sync_accounts_v2 WHERE user_id=NEW.user_id AND protocol=2 AND legacy_cleanup=0)
BEGIN SELECT RAISE(ABORT,'sync_upgrade_required'); END;
CREATE TRIGGER sync_v2_fence_legacy_key_update BEFORE UPDATE ON sync_keys
WHEN EXISTS(SELECT 1 FROM sync_accounts_v2 WHERE user_id=OLD.user_id AND protocol=2 AND legacy_cleanup=0)
  AND NOT EXISTS(SELECT 1 FROM account_deletions WHERE user_id=OLD.user_id)
BEGIN SELECT RAISE(ABORT,'sync_upgrade_required'); END;
