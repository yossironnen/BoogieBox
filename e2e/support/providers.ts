/**
 * A local stand-in for every online provider the server talks to (MusicBrainz, ListenBrainz, Wikidata, Wikipedia,
 * TheAudioDB, Deezer, Libre.fm, a ListenBrainz-compatible "own server", and lyrics/artwork sources that answer 404).
 * The server under test gets its `BOOGIEBOX_*_API_BASE` overrides pointed here, so a run never reaches the internet
 * and provider answers are the same every time. Each provider lives under its own path prefix.
 *
 * Every request is logged; specs read the log at `GET /__requests` and clear it with `DELETE /__requests`.
 */

import http from 'node:http'

export const PROVIDER_PORT = Number(process.env.BOOGIEBOX_E2E_PROVIDER_PORT || 3919)
export const PROVIDER_URL = `http://127.0.0.1:${PROVIDER_PORT}`

/** What the mock knows, for specs to assert on. Names and ids are fixed; nothing here is a real artist. */
export const MOCK = {
  auroraMbid: 'aa11aa11-1111-4111-8111-111111111111',
  neonTidesReleaseGroup: 'bb22bb22-2222-4222-8222-222222222222',
  auroraBio: 'Aurora Static is a fictional synth duo made for the BoogieBox test suite.',
  neonTidesText: 'Neon Tides is the first album by Aurora Static.',
  auroraListens: 4321,
  auroraListeners: 123,
  /** ListenBrainz top songs for Aurora Static: not titles in the fixture, so no popularity counts are stored. */
  auroraTopSongs: ['Signal Fire', 'Northern Wire', 'Tidal Choir', 'Static Bloom', 'Harbor Echo', 'Night Ferry'],
  velvetOwlsBio: 'The Velvet Owls are a fictional rock band.',
  miraDeezerId: 777,
  miraTopSongs: ['Kestrel Song One', 'Kestrel Song Two'],
  libreFmUser: 'e2e-libre',
  ownServerPath: '/own',
  ownServerToken: 'e2e-token',
  ownServerUser: 'e2e-maloja',
}

/** The env the server needs to use the mock for every provider. */
export function providerEnv(): Record<string, string> {
  return {
    BOOGIEBOX_MUSICBRAINZ_API_BASE: `${PROVIDER_URL}/mb`,
    BOOGIEBOX_LISTENBRAINZ_API_BASE: `${PROVIDER_URL}/lb`,
    BOOGIEBOX_LISTENBRAINZ_LABS_API_BASE: `${PROVIDER_URL}/labs`,
    BOOGIEBOX_WIKIDATA_API_BASE: `${PROVIDER_URL}/wd`,
    BOOGIEBOX_WIKIPEDIA_API_BASE: `${PROVIDER_URL}/wp`,
    BOOGIEBOX_THEAUDIODB_API_BASE: `${PROVIDER_URL}/tadb`,
    BOOGIEBOX_DEEZER_API_BASE: `${PROVIDER_URL}/dz`,
    BOOGIEBOX_LIBREFM_API_BASE: `${PROVIDER_URL}/librefm/2.0/`,
    BOOGIEBOX_DISCOGS_API_BASE: `${PROVIDER_URL}/discogs`,
    BOOGIEBOX_SPOTIFY_API_BASE: `${PROVIDER_URL}/spotify`,
    BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE: `${PROVIDER_URL}/spotify-accounts`,
    BOOGIEBOX_LRCLIB_API_BASE: `${PROVIDER_URL}/lrclib`,
    BOOGIEBOX_LYRICS_OVH_API_BASE: `${PROVIDER_URL}/lyricsovh`,
  }
}

export type LoggedRequest = { method: string; url: string; body: string }

type Answer = { status: number; json: unknown }

const ok = (json: unknown): Answer => ({ status: 200, json })
const NOT_FOUND: Answer = { status: 404, json: { error: 'Not Found' } }
const recordingId = (i: number) => `cc33cc33-3333-4333-8333-${String(i).padStart(12, '0')}`

function musicbrainz(path: string, query: URLSearchParams): Answer {
  const inc = query.get('inc') ?? ''
  if (path === '/artist/') {
    const q = query.get('query') ?? ''
    // `artist:"Name"` is the background identity search: no artist is identified that way, so only Aurora Static
    // (identified by its file tags) has a MusicBrainz id. `artist:(terms)` is Refresh Metadata.
    if (q.startsWith('artist:(') && /aurora/i.test(q)) {
      return ok({ artists: [{ id: MOCK.auroraMbid, name: 'Aurora Static', score: 100, disambiguation: 'e2e synth duo', area: { name: 'Iceland' } }] })
    }
    return ok({ artists: [] })
  }
  if (path === '/release-group/') {
    const q = query.get('query') ?? ''
    if (/neon/i.test(q)) {
      return ok({
        'release-groups': [{
          id: MOCK.neonTidesReleaseGroup, title: 'Neon Tides', score: 100, 'primary-type': 'Album',
          'first-release-date': '2021-03-01', 'artist-credit': [{ name: 'Aurora Static' }],
        }],
      })
    }
    return ok({ 'release-groups': [] })
  }
  if (path === '/recording/') return ok({ recordings: [] })
  if (path === `/artist/${MOCK.auroraMbid}` && inc === 'url-rels') {
    return ok({ relations: [{ type: 'wikidata', url: { resource: 'https://www.wikidata.org/wiki/Q4242' } }] })
  }
  if (path === `/release-group/${MOCK.neonTidesReleaseGroup}` && inc === 'url-rels') {
    return ok({ relations: [{ type: 'wikipedia', url: { resource: 'https://en.wikipedia.org/wiki/Neon_Tides' } }] })
  }
  // Tags and genres: none, so the fixture's styles and steering pills stay as they are.
  if (/^\/(artist|recording|release-group)\/[0-9a-f-]{36}$/.test(path) && inc.includes('tags')) return ok({ tags: [], genres: [] })
  if (/^\/(artist|release-group)\/[0-9a-f-]{36}$/.test(path)) return ok({ relations: [] })
  return NOT_FOUND
}

function listenbrainz(method: string, path: string, body: string): Answer {
  if (method === 'POST' && path === '/1/popularity/artist') {
    return ok(body.includes(MOCK.auroraMbid)
      ? [{ artist_mbid: MOCK.auroraMbid, total_listen_count: MOCK.auroraListens, total_user_count: MOCK.auroraListeners }]
      : [])
  }
  if (path === `/1/popularity/top-recordings-for-artist/${MOCK.auroraMbid}`) {
    return ok(MOCK.auroraTopSongs.map((name, i) => ({
      recording_mbid: recordingId(i + 1), recording_name: name, total_listen_count: 9000 - i * 1000,
    })))
  }
  if (path.startsWith('/1/popularity/top-recordings-for-artist/')) return ok([])
  return NOT_FOUND
}

function theaudiodb(path: string, query: URLSearchParams): Answer {
  if (path === '/search.php' && query.get('s') === 'The Velvet Owls') {
    return ok({ artists: [{ idArtist: '424242', strArtist: 'The Velvet Owls', strBiography: `${MOCK.velvetOwlsBio}\n\nThey exist only in tests.` }] })
  }
  if (path === '/search.php' || path === '/artist-mb.php') return ok({ artists: null })
  if (path === '/searchalbum.php' || path === '/album-mb.php') return ok({ album: null })
  return NOT_FOUND
}

function deezer(path: string, query: URLSearchParams): Answer {
  if (path === '/search/artist') {
    return ok({ data: query.get('q') === 'Mira Kestrel' ? [{ id: MOCK.miraDeezerId, name: 'Mira Kestrel' }] : [] })
  }
  if (path === `/artist/${MOCK.miraDeezerId}/top`) {
    return ok({ data: MOCK.miraTopSongs.map((title, i) => ({ title, link: `https://www.deezer.com/track/${i + 1}`, rank: 900000 - i })) })
  }
  if (/^\/artist\/\d+\/(top|related)$/.test(path)) return ok({ data: [] })
  return NOT_FOUND
}

function librefm(body: string): Answer {
  const form = new URLSearchParams(body)
  switch (form.get('method')) {
    case 'auth.getSession':
      return form.get('token') ? ok({ session: { name: MOCK.libreFmUser, key: 'e2e-session-key', subscriber: 0 } }) : ok({ error: 4, message: 'Invalid token' })
    case 'track.updateNowPlaying':
      return ok({ nowplaying: {} })
    case 'track.scrobble':
      return ok({ scrobbles: { '@attr': { accepted: 1, ignored: 0 } } })
    default:
      return ok({ error: 3, message: 'Invalid method' })
  }
}

function ownServer(method: string, path: string, headers: http.IncomingHttpHeaders): Answer {
  const token = String(headers.authorization ?? '').replace(/^Token\s+/i, '')
  if (path === '/1/validate-token') {
    return ok(token === MOCK.ownServerToken
      ? { code: 200, message: 'Token valid.', valid: true, user_name: MOCK.ownServerUser }
      : { code: 200, message: 'Token invalid.', valid: false })
  }
  if (method === 'POST' && path === '/1/submit-listens') {
    return token === MOCK.ownServerToken ? ok({ status: 'ok' }) : { status: 401, json: { code: 401, error: 'Invalid token' } }
  }
  return NOT_FOUND
}

function answer(method: string, url: URL, body: string, headers: http.IncomingHttpHeaders): Answer {
  const [, prefix = '', ...rest] = url.pathname.split('/')
  const path = `/${rest.join('/')}`
  const query = url.searchParams
  switch (prefix) {
    case 'mb': return musicbrainz(path, query)
    case 'lb': return listenbrainz(method, path, body)
    case 'labs': return path === '/similar-artists/json' ? ok([]) : NOT_FOUND
    case 'wd':
      return path === '/w/api.php' && query.get('ids') === 'Q4242'
        ? ok({ entities: { Q4242: { sitelinks: { enwiki: { title: 'Aurora Static' } } } } })
        : ok({ entities: {} })
    case 'wp':
      if (path === '/api/rest_v1/page/summary/Aurora_Static') {
        return ok({ type: 'standard', extract: MOCK.auroraBio, content_urls: { desktop: { page: 'https://en.wikipedia.org/wiki/Aurora_Static' } } })
      }
      if (path === '/api/rest_v1/page/summary/Neon_Tides') {
        return ok({ type: 'standard', extract: MOCK.neonTidesText, content_urls: { desktop: { page: 'https://en.wikipedia.org/wiki/Neon_Tides' } } })
      }
      return NOT_FOUND
    case 'tadb': return theaudiodb(path, query)
    case 'dz': return deezer(path, query)
    case 'librefm': return librefm(body)
    case MOCK.ownServerPath.slice(1): return ownServer(method, path, headers)
    default: return NOT_FOUND
  }
}

export async function startProviderMock(): Promise<http.Server> {
  const log: LoggedRequest[] = []
  const server = http.createServer((req, res) => {
    const chunks: Buffer[] = []
    req.on('data', (chunk: Buffer) => chunks.push(chunk))
    req.on('end', () => {
      const body = Buffer.concat(chunks).toString('utf8')
      const url = new URL(req.url ?? '/', PROVIDER_URL)
      const method = req.method ?? 'GET'
      if (url.pathname === '/__requests') {
        if (method === 'DELETE') log.length = 0
        res.writeHead(200, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify(method === 'DELETE' ? [] : log))
        return
      }
      log.push({ method, url: `${url.pathname}${url.search}`, body })
      const { status, json } = answer(method, url, body, req.headers)
      res.writeHead(status, { 'Content-Type': 'application/json' })
      res.end(JSON.stringify(json))
    })
  })
  await new Promise<void>((resolve, reject) => {
    server.once('error', (error: NodeJS.ErrnoException) => reject(error.code === 'EADDRINUSE'
      ? new Error(`Port ${PROVIDER_PORT} is in use. Stop whatever is using it or set BOOGIEBOX_E2E_PROVIDER_PORT.`)
      : error))
    server.listen(PROVIDER_PORT, '127.0.0.1', () => resolve())
  })
  return server
}

export async function stopProviderMock(server: http.Server | undefined): Promise<void> {
  if (!server) return
  server.closeAllConnections()
  await new Promise<void>((resolve) => server.close(() => resolve()))
}

/** For specs: the requests the server made to the mock since the last `clearProviderRequests`. */
export async function providerRequests(): Promise<LoggedRequest[]> {
  const res = await fetch(`${PROVIDER_URL}/__requests`)
  return (await res.json()) as LoggedRequest[]
}

export async function clearProviderRequests(): Promise<void> {
  await fetch(`${PROVIDER_URL}/__requests`, { method: 'DELETE' })
}
