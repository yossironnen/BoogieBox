/**
 * Defines a repository script for Seed Random Ratings.
 */

import { DatabaseSync } from 'node:sqlite';
import fs from 'node:fs';
import path from 'node:path';

const TOTAL_RATINGS = 500;
const RATING_VALUES = [0.5, 1, 1.5, 2, 2.5, 3, 3.5, 4, 4.5, 5];
const TYPES = [
  {
    name: 'tracks',
    table: 'track_ratings',
    entityTable: 'tracks',
    entityColumn: 'track_id',
  },
  {
    name: 'albums',
    table: 'album_ratings',
    entityTable: 'albums',
    entityColumn: 'album_id',
  },
  {
    name: 'artists',
    table: 'artist_ratings',
    entityTable: 'artists',
    entityColumn: 'artist_id',
  },
];

const dbPath = process.argv[2];

if (!dbPath) {
  console.error('Usage: seed-random-ratings.bat <path-to-boogiebox.db>');
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

function randomRating() {
  return RATING_VALUES[randomInt(RATING_VALUES.length)];
}

function shuffle(values) {
  const copy = [...values];
  for (let i = copy.length - 1; i > 0; i -= 1) {
    const j = randomInt(i + 1);
    [copy[i], copy[j]] = [copy[j], copy[i]];
  }
  return copy;
}

function buildPairs(users, entities, existingRatings) {
  const unratedPairs = [];
  const ratedPairs = [];
  for (const user of users) {
    for (const entity of entities) {
      const pair = { userId: user.id, entityId: entity.id };
      const key = `${user.id}\u0000${entity.id}`;
      if (existingRatings.has(key)) {
        ratedPairs.push(pair);
      } else {
        unratedPairs.push(pair);
      }
    }
  }
  return [...shuffle(unratedPairs), ...shuffle(ratedPairs)];
}

function loadExistingRatings(db, type) {
  const rows = db
    .prepare(`SELECT user_id, ${type.entityColumn} AS entity_id FROM ${type.table}`)
    .all();
  return new Set(rows.map((row) => `${row.user_id}\u0000${row.entity_id}`));
}

function allocateCounts(total, types) {
  const base = Math.floor(total / types.length);
  let remainder = total % types.length;
  return new Map(
    types.map((type) => {
      const count = base + (remainder > 0 ? 1 : 0);
      remainder -= 1;
      return [type.name, count];
    }),
  );
}

const db = new DatabaseSync(resolvedDbPath);
db.exec('PRAGMA foreign_keys = ON');
db.exec('PRAGMA busy_timeout = 15000');

const users = db.prepare('SELECT id, username FROM users ORDER BY username').all();

if (users.length === 0) {
  console.error('No users found. Create at least one user before seeding ratings.');
  db.close();
  process.exit(1);
}

const entityRows = new Map(
  TYPES.map((type) => [
    type.name,
    db.prepare(`SELECT id FROM ${type.entityTable} ORDER BY id`).all(),
  ]),
);

const missingTypes = TYPES.filter((type) => (entityRows.get(type.name) ?? []).length === 0);
if (missingTypes.length > 0) {
  console.error(
    `Cannot seed ratings: no ${missingTypes.map((type) => type.name).join(', ')} found.`,
  );
  db.close();
  process.exit(1);
}

const targetCounts = allocateCounts(TOTAL_RATINGS, TYPES);
const statements = new Map(
  TYPES.map((type) => [
    type.name,
    db.prepare(`
      INSERT INTO ${type.table} (user_id, ${type.entityColumn}, rating, updated_at)
      VALUES (?, ?, ?, datetime('now'))
      ON CONFLICT(user_id, ${type.entityColumn}) DO UPDATE SET
        rating = excluded.rating,
        updated_at = datetime('now')
    `),
  ]),
);

db.exec('BEGIN');

try {
  const results = new Map();

  for (const type of TYPES) {
    const entities = entityRows.get(type.name);
    const pairs = buildPairs(users, entities, loadExistingRatings(db, type));
    const target = targetCounts.get(type.name);
    const statement = statements.get(type.name);

    for (let i = 0; i < target; i += 1) {
      const pair = pairs[i % pairs.length];
      statement.run(pair.userId, pair.entityId, randomRating());
    }

    results.set(type.name, target);
  }

  db.exec('COMMIT');

  for (const type of TYPES) {
    console.log(`Seeded ${results.get(type.name)} ${type.name} rating operations.`);
  }
  console.log(`Done. Added or updated ${TOTAL_RATINGS} ratings across ${users.length} user(s).`);
} catch (error) {
  db.exec('ROLLBACK');
  console.error('Seed failed, rolled back:', error);
  process.exitCode = 1;
} finally {
  db.close();
}
