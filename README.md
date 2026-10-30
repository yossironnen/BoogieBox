# BoogieBox

BoogieBox is a self-hosted music library and player for Windows and Linux designed for collectors, audiophiles, and anyone with a large local music collection.

Enjoy lightning-fast browsing, powerful search, playlists and smart playlists, lyrics, visualizations, and personalized multi-user experiences. BoogieBox helps you rediscover your library instead of letting it sit untouched on a drive.

Its standout feature, BoogieMix, is an experimental AI-powered DJ system that turns ordinary playlists into seamless mixes by analyzing tracks and planning smooth transitions between songs.

Also includes waveform and BPM analysis, mobile-friendly access, Subsonic/OpenSubsonic app support, Last.fm and ListenBrainz scrobbling, internet radio, optional DLNA/UPnP streaming, and rich metadata and artwork enrichment.

---

## 📸 Screenshots

### Home

![BoogieBox home dashboard](docs/Screenshots/home_screen1.png)

![BoogieBox listening dashboard](docs/Screenshots/home_screen2.png)

### Browse Albums

![Album browser](docs/Screenshots/album_browse.png)

### Artist Page

![Artist detail page](docs/Screenshots/artist_page.png)

### BoogieMix

![BoogieMix](docs/Screenshots/boogiemix.png)

### Karaoke Playback

![Karaoke playback](docs/Screenshots/karaoke_playback.png)

### Play On

![Play On picker listing This browser, a WiiM Ultra, and an offline smart TV as playback targets](docs/Screenshots/remote_playback.png)

### Vinyl Mode

![Vinyl Mode](docs/Screenshots/vinyl_mode.png)

### Parametric EQ

![Parametric equalizer](docs/Screenshots/param_eq.png)

### Mobile Playback

![Mobile playback](docs/Screenshots/mobile_playback.png)

### Themes

![BoogieBox themes](docs/Screenshots/themes.png)

### Library Settings

![Library settings](docs/Screenshots/lib_settings.png)

---

## Why BoogieBox?

- Built for people who own and curate their music collections
- Multi-user support with individual preferences and ratings
- AI-powered BoogieMix DJ engine
- Vinyl Mode plays your music with an analog look and sound (yeah, some crackles & pops)
- Karaoke lyrics and waveform navigation
- Genre Galaxy music discovery
- Mobile-friendly interface
- Play On: send playback to WiiM streamers, smart TVs, and other UPnP/DLNA speakers, and move it between devices mid-song
- Subsonic apps: use native phone and desktop players such as Amperfy with your library, and download music for offline listening
- DLNA/UPnP streaming support
- Fully self-hosted with no subscriptions

---

## 🤖 Meet BoogieMix

BoogieMix is BoogieBox's experimental AI DJ.

Instead of simply shuffling tracks, BoogieMix analyzes your music and creates continuous listening sessions by intelligently planning transitions between songs.

Whether you're creating a workout mix, a party playlist, or a long background listening session, BoogieMix aims to make your playlists feel more like a professionally mixed DJ set.

### Features

- Intelligent transition planning
- AI-assisted mix generation
- Optional deep-analysis mode for advanced track matching
- Fully local processing
- Downloadable rendered mixes

---

## ✨ Highlights

### 🎵 Your Music, Your Way

- Organize and browse music across multiple folders and drives
- Fast search, playlists, Favourites, and personalized listening history with per-user play counts
- Import M3U/M3U8 playlists from other players, with a preview of what matched and folder swaps for music that lives somewhere else now
- Individual user profiles with avatars, optional PIN protection, and personal preferences
- Rate artists, albums, and tracks with half-star precision

### 🤖 Experimental AI DJ Features

- BoogieMix automatically plans transitions between tracks
- AI-assisted mix generation for smoother playlist playback
- Optional deep-analysis mode for advanced track matching and transition planning
- Early-stage DJ capabilities that continue to evolve with each release

### 🎧 Rich Listening Experience

- Full-featured music player with queue management, shuffle, repeat, and smooth transitions
- Interactive waveform seek bar with Sonic Fingerprint stem-analysis overlay for precise navigation
- Sonic Fingerprint panel showing per-stem energy, vocal/drum/bass presence, and transition windows from deep analysis
- Synchronized lyrics and karaoke mode
- Vinyl Mode for a classic turntable-inspired experience
- 7-band parametric EQ with presets, custom profiles, and automatic artist-based EQ matching

### 🌌 Discover More Music

- Smart playlists pick tracks by rules you set (genre, rating, plays, date added and more) and stay up to date on their own; start one from a genre or an idea on the home screen
- Genre Galaxy visualizes relationships between your favorite genres and artists
- Artist insights powered by Last.fm
- Personalized home screen featuring listening trends, statistics, and recommendations

### 📱 Mobile Friendly

- Lightweight mobile experience designed for quick browsing and playback
- Browse artists, albums, playlists, and search your library from your phone
- Dedicated Now Playing screen with full playback controls

### 🖼️ Artwork, Metadata & Library Enrichment

- Automatic artwork and metadata enhancement from multiple online sources
- Manual artwork management when you want complete control
- Background analysis for BPM, waveforms, and other music insights
- Extensive caching for fast browsing and responsive playback

### 🔗 Connect Your Music Ecosystem

- Subsonic/OpenSubsonic apps on your phone or desktop (see [Subsonic Apps](#-subsonic-apps))
- Scrobbling to each user's own Last.fm and ListenBrainz accounts, from the web player, Play On speakers and Subsonic apps
- Last.fm artist information
- Internet radio stations, played in the browser and listed in Subsonic apps
- Lyrics integration
- DLNA/UPnP streaming for compatible devices
- Optional integrations for artwork, metadata enrichment, and advanced music analysis

### 🏠 Built for Self-Hosting

- Server can run on Windows or Linux
- Simple first-run setup
- Connect to free metadata providers
- Multi-user support
- Local and network library support
- Scheduled backups and one-click restore, including moving to a new machine
- Optional Windows desktop application with automatic server discovery (in dev)

---

## 📊 Feature Summary

| Feature | Supported |
|----------|----------|
| Multi-user Profiles | ✅ |
| Playlists | ✅ |
| Karaoke Lyrics | ✅ |
| Waveform Navigation | ✅ |
| Smart Playlists | ✅ |
| BoogieMix AI Mixing (experimental!) | ✅ |
| Genre Galaxy | ✅ |
| Mobile Interface | ✅ |
| DLNA/UPnP Streaming | ✅ |
| Play On (UPnP/DLNA speakers) | ✅ |
| Subsonic/OpenSubsonic Apps | ✅ |
| Last.fm & ListenBrainz Scrobbling | ✅ |
| Internet Radio | ✅ |
| Last.fm Integration | ✅ |
| Artwork Enrichment | ✅ |
| Sonic Fingerprint (stem analysis) | ✅ |
| Local Music Libraries | ✅ |

---

## 📲 Subsonic Apps

BoogieBox speaks the Subsonic/OpenSubsonic API, so apps built for Subsonic servers can browse and play your library on your home network.

- **Turn it on:** an admin enables it in Settings → Advanced → Subsonic apps. It is off by default.
- **Sign in:** each user sets a Subsonic password in User Settings → Connected apps (it is separate from the PIN). Apps that support OpenSubsonic API keys can use a key created there instead. Connected apps also shows the server address and username to enter.
- **What works:** browsing, search, streaming (original file, or MP3, Opus or AAC at a lower bitrate), offline downloads, cover art, playlists, stars (BoogieBox Favourites), ratings, lyrics, Instant Mix, scrobbling, now playing, internet radio, a play queue that follows you between apps, library rescans, your rendered BoogieMix mixes, and a jukebox mode that plays on a Play On speaker.
- **Tested with:** Amperfy (iOS). Other Subsonic apps such as Symfonium, Feishin and DSub should work but have not been tested yet.
- **Away from home:** download music in the app while you are on your home network. BoogieBox is not meant to be reachable from the internet (see the Security Notice).

Method-level details are in [`docs/API.md`](docs/API.md#subsonic-apps-opensubsonic-rest).

---

## 🎼 Supported Formats

**Music:** MP3, FLAC, M4A/MP4 Audio, OGG, OPUS, WAV, AAC, WMA, AIFF

---

## 🛠️ Built With

- Rust
- React
- TypeScript
- Tauri 2
- SQLite
- FFmpeg

---

## 🚀 Installation

### Windows

1. Download the latest release.
2. Run the installer.
3. Launch BoogieBox.
4. Follow the first-run setup wizard.
5. Start enjoying your music.

### Linux (server only)

BoogieBox runs on Linux as a headless server. Use any browser on the same network to access the UI.

#### From a release tarball (recommended)

1. Download the latest `boogiebox-*-linux-rs.tar.gz` from the [Releases](https://github.com/yossironnen/BoogieBox/releases) page.
2. Extract and install:

```bash
tar -xzf boogiebox-*-linux-rs.tar.gz
cd boogiebox-*-linux-rs
sudo ./install/install.sh
```

3. Open `http://localhost:3001` in a browser and complete the setup wizard.

#### From source

```bash
git clone https://github.com/yossironnen/BoogieBox
cd boogiebox
./build-server-rust.sh --no-test
sudo ./Releases/boogiebox-*/install/install.sh
```

#### Non-systemd / manual run

```bash
BOOGIEBOX_CONFIG_DIR=~/.config/boogiebox ./boogiebox-server
```

Then open `http://localhost:3001` in a browser to complete first-run setup.

### Developers

See the documentation below for development setup and build instructions.

### Backup and restore

Settings › Advanced › Backup (admins) backs up the database weekly at 03:30 by default and keeps the last 4.
Each backup is a folder `boogiebox-<date>-<time>-v<version>` holding the database, the key that protects Subsonic
and scrobbling passwords, and artwork you uploaded or picked. Music files are never copied. BoogieBox also keeps a
copy of the database before every upgrade (the last 2).

- **Keep the backup folder private**: it holds the password key. A folder on another disk survives a disk failure.
- **Restore** from the list, or "Restore from another folder…". Your current data is saved first to
  `pre-restore-<date>-<time>` in the data folder, and everyone signs in again.
- **New machine**: copy a backup folder over, start BoogieBox, choose **Restore from a backup** on the first-run
  screen, and point each music folder at its new location.
- **Without the UI** (stop the server first):
  `boogiebox-server --list-backups [--path DIR]` and
  `boogiebox-server --restore <backup folder> [--data-folder DIR] [--remap OLD=NEW]...`

---

## 🔒 Security Notice

BoogieBox is designed for local-network use.

It has not been hardened or tested for public internet exposure. Do not expose BoogieBox directly to the internet.

This includes Subsonic apps: they send your Subsonic password or API key over plain HTTP. Use them on your home network and download music for listening elsewhere.

---

## 📚 Documentation

### User Documentation

- Installation Guide
- User Guide
- BoogieMix Guide

### Developer Documentation

- `docs/Architecture.md`
- `docs/API.md`
- `docs/Development.md`
- `docs/ProjectStructure.md`

---

## ❤️ Why I Built BoogieBox

Most modern music platforms focus on streaming. BoogieBox focuses on helping you enjoy and rediscover the music you already own.

Whether you're managing a carefully curated FLAC collection, building playlists for every mood, singing along with karaoke lyrics, or experimenting with AI-generated mixes through BoogieMix, BoogieBox is designed to make local music collections feel alive again.

---

## Support BoogieBox

BoogieBox is developed independently and offered free for self-hosting.

If you enjoy the project and would like to support its continued development, you can [support BoogieBox on Ko-fi](https://ko-fi.com/yronnen).
