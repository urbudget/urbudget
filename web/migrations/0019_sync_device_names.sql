-- What each device that syncs is called and when it last asked, so Account & sync can list an
-- account's devices. The name is the one the device gives when it registers (its computer name,
-- or its system for a phone). Both NULL for a device that registered before this.
ALTER TABLE sync_installations_v2 ADD COLUMN device TEXT;
ALTER TABLE sync_installations_v2 ADD COLUMN last_seen_at TEXT;
