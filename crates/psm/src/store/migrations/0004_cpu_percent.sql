-- %CPU over the sampling window right before the snapshot; NULL when not sampled.
ALTER TABLE processes ADD COLUMN cpu_percent REAL;
