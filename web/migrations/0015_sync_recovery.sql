-- A recovery code's copy of the account's sync key: the key sealed with a key made from a code
-- only the person holds, and the salt that code is stretched with. Both NULL for an account
-- that has no recovery code (its key is made from a passphrase, which recovers it).
-- An account whose key isn't made from a passphrase has `salt` NULL and always has these.
ALTER TABLE sync_accounts_v2 ADD COLUMN recovery_salt TEXT;
ALTER TABLE sync_accounts_v2 ADD COLUMN recovery_wrapped TEXT;
