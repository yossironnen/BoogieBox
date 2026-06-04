/**
 * Defines a repository script for Seed Random Playback History.
 */

import { DatabaseSync } from 'node:sqlite';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const PLAYS_PER_USER = 500;
const DAYS_BACK = 30;

const dbPath = process.argv[2];

if (!dbPath) {
  console.error('Usage: seed-random-playback-history.bat <path-to-boogiebox.db>');
  process.exit(1);
}

const resolvedDbPath = path.resolve(dbPath);

if (!fs.existsSync(resolvedDbPath)) {
  console.error(`Database not found: ${resolvedDbPath}`);
  process.exit(1);
}

function randomInt(maxExclusive) {
  return Math.floor(Math.random() * maxExclusive);
}

function randomPlayedAt() {
  const now = Date.now();
  const spanMs = DAYS_BACK * 24 * 60 * 60 * 1000;
  const playedAt = new Date(now - Math.random() * spanMs);
  return playedAt.toISOString().replace('T', ' ').slice(0, 19);
}

const db = new DatabaseSync(resolvedDbPath);
db.exec('PRAGMA foreign_keys = ON');
db.exec('PRAGMA busy_timeout = 15000');

const users = db.prepare('SELECT id, username FROM users ORDER BY username').all();
const tracks = db.prepare('SELECT id, artist_id FROM tracks ORDER BY id').all();

if (users.length === 0) {
  console.error('No users found. Create at least one user before seeding playback history.');
  db.close();
  process.exit(1);
}

if (tracks.length === 0) {
  console.error('No tracks found. Scan a music library before seeding playback history.');
  db.close();
  process.exit(1);
}

const insertHistory = db.prepare(`
  INSERT INTO play_history (id, user_id, track_id, played_at)
  VALUES (?, ?, ?, ?)
`);

const updateTrackStats = db.prepare(`
  UPDATE tracks
  SET play_count = play_count + ?,
      last_played_at = (
        SELECT MAX(played_at)
        FROM play_history
        WHERE track_id = tracks.id
      )
  WHERE id = ?
`);

const updateArtistStats = db.prepare(`
  UPDATE artists
  SET play_count = play_count + ?
  WHERE id = ?
`);

db.exec('BEGIN');

try {
  for (const user of users) {
    const trackCounts = new Map();
    const artistCounts = new Map();

    for (let i = 0; i < PLAYS_PER_USER; i += 1) {
      const track = tracks[randomInt(tracks.length)];
      insertHistory.run(crypto.randomUUID(), user.id, track.id, randomPlayedAt());
      trackCounts.set(track.id, (trackCounts.get(track.id) ?? 0) + 1);

      if (track.artist_id) {
        artistCounts.set(track.artist_id, (artistCounts.get(track.artist_id) ?? 0) + 1);
      }
    }

    for (const [trackId, count] of trackCounts) {
      updateTrackStats.run(count, trackId);
    }

    for (const [artistId, count] of artistCounts) {
      updateArtistStats.run(count, artistId);
    }

    console.log(`Seeded ${PLAYS_PER_USER} plays for ${user.username} (${user.id}).`);
  }

  db.exec('COMMIT');
  console.log(`Done. Added ${users.length * PLAYS_PER_USER} plays across ${users.length} user(s).`);
} catch (error) {
  db.exec('ROLLBACK');
  console.error('Seed failed, rolled back:', error);
  process.exitCode = 1;
} finally {
  db.close();
}
