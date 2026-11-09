-- Language chosen in the profile ('fr' | 'en' | 'es'); NULL follows the browser.
ALTER TABLE users ADD COLUMN language TEXT CHECK (language IN ('fr', 'en', 'es'));

-- The activity feed is written in every language, each reader gets theirs. `message` is the
-- French version (older events only have it).
ALTER TABLE events ADD COLUMN message_en TEXT;
ALTER TABLE events ADD COLUMN message_es TEXT;
