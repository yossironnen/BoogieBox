/**
 * Defines a repository script for Seed Casual Playlists.
 * Creates a couple of casually-named playlists from a given library's tracks.
 */

import { DatabaseSync } from 'node:sqlite';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const LIBRARY_NAME = 'Music';
const PLAYLIST_NAMES = ['Rainy Day Vibes', 'Late Night Drive'];
const MIN_TRACKS = 15;
const MAX_TRACKS = 20;

const dbPath = process.argv[2];

if (!dbPath) {
  console.error('Usage: node seed-casual-playlists.mjs <path-to-boogiebox.db>');
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

function shuffle(values) {
  const copy = [...values];
  for (let i = copy.length - 1; i > 0; i -= 1) {
    const j = randomInt(i + 1);
    [copy[i], copy[j]] = [copy[j], copy[i]];
  }
  return copy;
}

const db = new DatabaseSync(resolvedDbPath);
db.exec('PRAGMA foreign_keys = ON');
db.exec('PRAGMA busy_timeout = 15000');

const library = db.prepare('SELECT id, name FROM libraries WHERE name = ?').get(LIBRARY_NAME);
if (!library) {
  console.error(`Library not found: ${LIBRARY_NAME}`);
  db.close();
  process.exit(1);
}

const user = db.prepare('SELECT id, username FROM users ORDER BY username').get();
if (!user) {
  console.error('No users found. Create at least one user before seeding playlists.');
  db.close();
  process.exit(1);
}

const tracks = db.prepare('SELECT id FROM tracks WHERE library_id = ?').all(library.id);
if (tracks.length === 0) {
  console.error(`No tracks found in library "${LIBRARY_NAME}".`);
  db.close();
  process.exit(1);
}

const insertPlaylist = db.prepare(`
  INSERT INTO playlists (id, user_id, name, description, created_at, updated_at, remember_progress)
  VALUES (?, ?, ?, ?, datetime('now'), datetime('now'), 0)
`);

const insertPlaylistTrack = db.prepare(`
  INSERT INTO playlist_tracks (id, playlist_id, track_id, position, added_at)
  VALUES (?, ?, ?, ?, datetime('now'))
`);

db.exec('BEGIN');

try {
  for (const name of PLAYLIST_NAMES) {
    const playlistId = crypto.randomUUID();
    insertPlaylist.run(playlistId, user.id, name, null);

    const count = MIN_TRACKS + randomInt(MAX_TRACKS - MIN_TRACKS + 1);
    const picked = shuffle(tracks).slice(0, Math.min(count, tracks.length));

    picked.forEach((track, index) => {
      insertPlaylistTrack.run(crypto.randomUUID(), playlistId, track.id, index);
    });

    console.log(`Created playlist "${name}" with ${picked.length} tracks.`);
  }

  db.exec('COMMIT');
  console.log(`Done. Created ${PLAYLIST_NAMES.length} playlists for ${user.username} from "${LIBRARY_NAME}".`);
} catch (error) {
  db.exec('ROLLBACK');
  console.error('Seed failed, rolled back:', error);
  process.exitCode = 1;
} finally {
  db.close();
}
