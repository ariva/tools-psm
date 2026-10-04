-- Free text given with `psm snap`, shown next to the label.
ALTER TABLE snapshots ADD COLUMN description TEXT;
