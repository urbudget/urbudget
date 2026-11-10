-- Where a head moved over from protocol 1 came from: the incarnation the move gave it and the
-- v1 revision it carried, as {"incarnation":"...","revision":"..."}. Written once by that move and
-- never changed. A device still holding the budget's v1 history can link its copy for as long
-- as the head keeps that incarnation, however often it has been rewritten since. A restore
-- gives the head a new incarnation, and the origin then no longer matches it.
ALTER TABLE sync_heads_v2 ADD COLUMN legacy TEXT;
