UPDATE entries
SET value = REPLACE(value, '"', '')
WHERE stat_kind = "rules" AND file_kind = "rlx";
