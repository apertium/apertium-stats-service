UPDATE entries
SET value = CAST(value AS INTEGER)
WHERE stat_kind = "Rules";
