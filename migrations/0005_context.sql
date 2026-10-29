ALTER TABLE messages DROP CONSTRAINT messages_depth_check;
ALTER TABLE messages DROP CONSTRAINT messages_ancestors_check;
ALTER TABLE messages ADD CONSTRAINT messages_depth_check CHECK (depth BETWEEN 0 AND 4096);
ALTER TABLE messages ADD CONSTRAINT messages_ancestors_check CHECK (cardinality(ancestors) <= 4096);
ALTER TABLE generations ADD COLUMN context jsonb;
