-- The Android test takes its testers from the public Google Group urbudget-testers now, so the
-- website's email sign-up and its admin list are gone. The addresses left before it are deleted.
-- The table stays, empty, because migrations are never rolled back; nothing reads or writes it.
DELETE FROM android_testers;
