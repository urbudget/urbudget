-- Where a head moved over from protocol 1 came from: the incarnation the move gave it and the
-- v1 revision it carried, as {"incarnation":"...","revision":"..."}. Written by the move and
-- never changed after. A device still holding the budget's v1 history links its copy for as long
-- as the head keeps that incarnation, however often other devices have rewritten it since. A
-- restore gives the head a new incarnation, and the origin then no longer matches it.
ALTER TABLE sync_heads_v2 ADD COLUMN legacy TEXT;

-- Heads moved before this column existed (sync v2 went live on 22 November 2026). The move is the
-- only writer of an incarnation of 32 lowercase hex digits, lower(hex(randomblob(16))); the apps
-- make theirs with budgets::new_id(), which is 25 characters. Such a head still has the
-- incarnation the move gave it. Its v1 revision then is no longer known once another device has
-- rewritten it, so the version it has now stands in: never earlier than that revision, which is
-- all a device's link checks (its own last v1 revision can be no later).
UPDATE sync_heads_v2 SET legacy=json_object('incarnation',incarnation,'revision',version)
WHERE legacy IS NULL AND length(incarnation)=32 AND incarnation NOT GLOB '*[^0-9a-f]*';
