-- What an account's sync key is made from: 'code' for a recovery code the app made, NULL for a
-- passphrase someone chose (every key from before this column). The key is made the same way
-- from either, with `salt`; this only tells a device what to ask for.
ALTER TABLE sync_accounts_v2 ADD COLUMN key_secret TEXT;
-- A replacement recovery code's copy of the key: the key sealed with a key made from a code
-- only the person holds, and the salt that code is stretched with. Both NULL until someone
-- replaces a lost recovery code or passphrase, and again after a reset.
ALTER TABLE sync_accounts_v2 ADD COLUMN recovery_salt TEXT;
ALTER TABLE sync_accounts_v2 ADD COLUMN recovery_wrapped TEXT;
