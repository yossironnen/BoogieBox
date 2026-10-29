/**
 * A playlist file for the import specs: one line of every kind the matcher handles, against the fixture library.
 */

import fs from 'node:fs'
import path from 'node:path'
import { MUSIC_DIR, RUN_DIR } from './paths.ts'

/** Titles the import should add, in file order. */
export const IMPORTED_TITLES = ['Harbor Lights', 'Ink Sky', 'Blue Hour Waltz', 'Window Seat']

/** 7 entries: exact path, relative path, a Windows path from another machine, an #EXTINF with no path,
 * a repeat, a web link and a missing file. Expect Found 4, Not found 1, Web links 1, Repeats 1. */
export function writeImportFixture(fileName: string): string {
  const exact = path.join(MUSIC_DIR, 'Aurora Static', 'Neon Tides', '01 Harbor Lights.mp3')
  const lines = [
    '#EXTM3U',
    '#EXTINF:30,Aurora Static - Harbor Lights',
    exact,
    '../The Velvet Owls/Paper Moons/02 Ink Sky.mp3',
    'Z:\\Old PC\\Music\\Mira Kestrel\\Quiet Hours\\01 Blue Hour Waltz.m4a',
    '#EXTINF:30,Mira Kestrel - Window Seat',
    '#EXTINF:30,Aurora Static - Harbor Lights',
    exact,
    'https://radio.example/live.mp3',
    '#EXTINF:200,Nobody - Missing Song',
    'Z:\\Old PC\\Music\\Nobody\\Missing Song.mp3',
  ]
  const dir = path.join(RUN_DIR, 'imports')
  fs.mkdirSync(dir, { recursive: true })
  const file = path.join(dir, fileName)
  fs.writeFileSync(file, `${lines.join('\r\n')}\r\n`)
  return file
}
