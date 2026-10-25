/**
 * Builds the small, deterministic music library the UI release suite scans.
 * Tones are generated with FFmpeg, so no copyrighted audio is needed. Output is cached under tmp/e2e-fixtures.
 */

import { execFileSync } from 'node:child_process'
import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

export type FixtureTrack = { title: string; frequency: number }
export type FixtureAlbum = {
  artist: string
  album: string
  year: number
  genre: string
  format: 'mp3' | 'flac' | 'm4a'
  coverColor: string
  /** `folder` writes folder.jpg beside the tracks; `embedded` puts the cover inside each file. */
  art: 'folder' | 'embedded'
  tracks: FixtureTrack[]
}

export const TRACK_SECONDS = 30

export const ALBUMS: FixtureAlbum[] = [
  {
    artist: 'Aurora Static',
    album: 'Neon Tides',
    year: 2021,
    genre: 'Electronic',
    format: 'mp3',
    coverColor: '0x2a6fdb',
    art: 'folder',
    tracks: [
      { title: 'Harbor Lights', frequency: 220 },
      { title: 'Glass Current', frequency: 262 },
      { title: 'Low Tide Radio', frequency: 294 },
    ],
  },
  {
    artist: 'Aurora Static',
    album: 'Night Signals',
    year: 2023,
    genre: 'Electronic',
    format: 'flac',
    coverColor: '0x7b2adb',
    art: 'folder',
    tracks: [
      { title: 'Antenna Bloom', frequency: 330 },
      { title: 'Static Lullaby', frequency: 349 },
    ],
  },
  {
    artist: 'The Velvet Owls',
    album: 'Paper Moons',
    year: 2019,
    genre: 'Rock',
    format: 'mp3',
    coverColor: '0xdb6f2a',
    art: 'embedded',
    tracks: [
      { title: 'Cardboard Comet', frequency: 392 },
      { title: 'Ink Sky', frequency: 440 },
      { title: 'Folded Orbit', frequency: 494 },
    ],
  },
  {
    artist: 'Mira Kestrel',
    album: 'Quiet Hours',
    year: 2020,
    genre: 'Jazz',
    format: 'm4a',
    coverColor: '0x2adb8f',
    art: 'folder',
    tracks: [
      { title: 'Blue Hour Waltz', frequency: 523 },
      { title: 'Window Seat', frequency: 587 },
    ],
  },
]

export const ARTISTS = [...new Set(ALBUMS.map((album) => album.artist))]
export const TRACK_COUNT = ALBUMS.reduce((sum, album) => sum + album.tracks.length, 0)

const CODEC_ARGS: Record<FixtureAlbum['format'], string[]> = {
  mp3: ['-c:a', 'libmp3lame', '-b:a', '128k', '-id3v2_version', '3'],
  flac: ['-c:a', 'flac'],
  m4a: ['-c:a', 'aac', '-b:a', '128k'],
}

function safeName(value: string): string {
  return value.replace(/[^A-Za-z0-9 ._-]/g, '_')
}

function manifestHash(): string {
  return crypto.createHash('sha256').update(JSON.stringify({ ALBUMS, TRACK_SECONDS, v: 2 })).digest('hex')
}

function generate(ffmpeg: string, outDir: string): void {
  fs.rmSync(outDir, { recursive: true, force: true })
  for (const album of ALBUMS) {
    const albumDir = path.join(outDir, safeName(album.artist), safeName(album.album))
    fs.mkdirSync(albumDir, { recursive: true })
    const cover = path.join(album.art === 'folder' ? albumDir : outDir, album.art === 'folder' ? 'folder.jpg' : 'embedded-cover.jpg')
    execFileSync(ffmpeg, [
      '-hide_banner', '-loglevel', 'error', '-y',
      '-f', 'lavfi', '-i', `color=c=${album.coverColor}:s=500x500:d=1`,
      '-frames:v', '1', cover,
    ])
    album.tracks.forEach((track, index) => {
      const file = path.join(albumDir, `${String(index + 1).padStart(2, '0')} ${safeName(track.title)}.${album.format}`)
      const coverArgs = album.art === 'embedded'
        ? ['-i', cover, '-map', '0:a', '-map', '1:v', '-c:v', 'mjpeg', '-disposition:v', 'attached_pic']
        : []
      execFileSync(ffmpeg, [
        '-hide_banner', '-loglevel', 'error', '-y',
        '-f', 'lavfi', '-i', `sine=frequency=${track.frequency}:duration=${TRACK_SECONDS}`,
        ...coverArgs,
        '-metadata', `title=${track.title}`,
        '-metadata', `artist=${album.artist}`,
        '-metadata', `album_artist=${album.artist}`,
        '-metadata', `album=${album.album}`,
        '-metadata', `track=${index + 1}/${album.tracks.length}`,
        '-metadata', `date=${album.year}`,
        '-metadata', `genre=${album.genre}`,
        ...CODEC_ARGS[album.format],
        file,
      ])
    })
    if (album.art === 'embedded') fs.rmSync(cover)
  }
}

/** Generates the library into the cache once, then copies it into `targetDir` (scans may write artwork beside it). */
export function prepareLibrary(ffmpeg: string, cacheDir: string, targetDir: string): void {
  const marker = path.join(cacheDir, 'manifest.sha256')
  const hash = manifestHash()
  const cachedMusic = path.join(cacheDir, 'music')
  if (!fs.existsSync(marker) || fs.readFileSync(marker, 'utf8') !== hash) {
    try {
      generate(ffmpeg, cachedMusic)
    } catch (error) {
      throw new Error(`Generating fixture audio with ${ffmpeg} failed: ${(error as Error).message}`, { cause: error })
    }
    fs.writeFileSync(marker, hash)
  }
  fs.rmSync(targetDir, { recursive: true, force: true })
  fs.cpSync(cachedMusic, targetDir, { recursive: true })
}
