-- People who asked, on the home page, to test the Android app before it's on Google Play. Only the
-- address they gave and when; the admin page lists them to add to Play's closed test by hand.
-- Deleted from the admin page, on request, or all at once when the app is on Google Play.
CREATE TABLE android_testers (
  email TEXT PRIMARY KEY CHECK (length(email) BETWEEN 3 AND 254),
  created_at TEXT NOT NULL
);
