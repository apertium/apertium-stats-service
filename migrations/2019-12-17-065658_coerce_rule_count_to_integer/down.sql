UPDATE entries
SET value = ('"' || value || '"')
WHERE stat_kind = "rules" AND file_kind = "rlx";
