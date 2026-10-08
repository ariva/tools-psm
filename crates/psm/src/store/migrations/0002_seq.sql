-- Snapshot numbers within a session: 0 is the baseline, then 1, 2, ...
-- Stored (never recomputed) so deleting a snapshot leaves the others alone.
ALTER TABLE snapshots ADD COLUMN seq INTEGER NOT NULL DEFAULT 0;
UPDATE snapshots SET seq = (
    SELECT count(*) FROM snapshots p
    WHERE p.session_id = snapshots.session_id AND p.id < snapshots.id
);
CREATE UNIQUE INDEX idx_snapshots_seq ON snapshots(session_id, seq);
