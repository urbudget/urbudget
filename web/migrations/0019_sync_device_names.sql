-- What each device that syncs is called and when it last asked, so Account & sync can list an
-- account's devices. The name is the one the device gives when it registers (its computer name,
-- or its system for a phone). Both NULL for a device that registered before this.
ALTER TABLE sync_installations_v2 ADD COLUMN device TEXT;
ALTER TABLE sync_installations_v2 ADD COLUMN last_seen_at TEXT;
-- 1 from when another device resets sync until this one syncs again, with the new key: it can't
-- read the account's budgets meanwhile, so the list leaves it out.
ALTER TABLE sync_installations_v2 ADD COLUMN cut_off INTEGER NOT NULL DEFAULT 0;
