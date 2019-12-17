UPDATE entries
SET value = CAST(value AS TEXT)
WHERE stat_kind = "Rules";
