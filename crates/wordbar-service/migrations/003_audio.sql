-- Schema v3: remember where a word's pronunciation recording lives.
--
-- The dictionary already returned this and it was being thrown away, leaving
-- pronunciation to robotic speech synthesis.

ALTER TABLE words ADD COLUMN audio_url TEXT;
