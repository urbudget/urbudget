-- store_counts takes numbers from more than one store now, so each row says which: 'microsoft'
-- (Partner Center: 'acquisitions', 'installs'), 'play' (Play Console's monthly installs reports:
-- 'user_installs', 'device_installs') or 'appstore' (App Store Connect's daily sales summary:
-- 'downloads' for first-time downloads, 'redownloads', 'updates'). The rows already here are the
-- Microsoft Store's. SQLite can't change a primary key in place, so the table is copied.
CREATE TABLE store_counts_new (
  store TEXT NOT NULL DEFAULT 'microsoft',
  day TEXT NOT NULL,
  kind TEXT NOT NULL,
  count INTEGER NOT NULL,
  PRIMARY KEY (store, day, kind)
);
INSERT INTO store_counts_new (store, day, kind, count) SELECT 'microsoft', day, kind, count FROM store_counts;
DROP TABLE store_counts;
ALTER TABLE store_counts_new RENAME TO store_counts;

-- channel_fetches.source gains 'play' and 'appstore'; it is free text, so nothing changes there.
