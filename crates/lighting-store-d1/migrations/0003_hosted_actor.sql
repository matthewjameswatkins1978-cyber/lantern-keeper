-- LK-Q26 attributed capture: who-said-it on the note row itself.
-- Additive, nullable, no backfill. Receipts already bind principal; this
-- makes the note row self-describing for the contract matrix without
-- requiring a receipt join to answer "who captured this".
ALTER TABLE memory_item ADD COLUMN actor TEXT;
