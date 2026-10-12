#!/usr/bin/env node
/**
 * Lists UPnP/DLNA MediaRenderers on the local network and dumps what Play On needs to know about them.
 *
 * Usage:
 *   node scripts/upnp-probe.mjs                    list renderers (name, model, services, gapless support)
 *   node scripts/upnp-probe.mjs --save <dir>       also save device.xml, AVTransport SCPD and GetProtocolInfo per device
 *   node scripts/upnp-probe.mjs --wait <ms>        how long to collect M-SEARCH responses (default 3000)
 *
 * LAN only: multicast M-SEARCH, then plain HTTP to the addresses that answered. Nothing is changed on the devices
 * (only read-only GET requests and the GetProtocolInfo query are sent).
 */

import dgram from 'node:dgram';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const SSDP_ADDR = '239.255.255.250';
const SSDP_PORT = 1900;
const ST_RENDERER = 'urn:schemas-upnp-org:device:MediaRenderer:1';
const HTTP_TIMEOUT_MS = 3000;

const args = process.argv.slice(2);
const argValue = (flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};
const saveDir = argValue('--save');
const waitMs = Number(argValue('--wait') ?? 3000);

/** IPv4 addresses of non-internal interfaces (one M-SEARCH socket per interface). */
function lanAddresses() {
  const out = [];
  for (const [name, addrs] of Object.entries(os.networkInterfaces())) {
    for (const a of addrs ?? []) {
      if (a.family === 'IPv4' && !a.internal && !a.address.startsWith('169.254.')) out.push({ name, address: a.address });
    }
  }
  return out;
}

/** Parses an SSDP response's headers (case-insensitive) into a lower-cased map. */
function parseSsdp(text) {
  const headers = {};
  for (const line of text.split(/\r?\n/).slice(1)) {
    const idx = line.indexOf(':');
    if (idx > 0) headers[line.slice(0, idx).trim().toLowerCase()] = line.slice(idx + 1).trim();
  }
  return headers;
}

/** Sends M-SEARCH on every LAN interface and resolves with unique LOCATION URLs. */
function search() {
  return new Promise((resolve) => {
    const found = new Map();
    const sockets = [];
    const msg = Buffer.from(
      `M-SEARCH * HTTP/1.1\r\nHOST: ${SSDP_ADDR}:${SSDP_PORT}\r\nMAN: "ssdp:discover"\r\nMX: 2\r\nST: ${ST_RENDERER}\r\n\r\n`,
    );
    for (const iface of lanAddresses()) {
      const sock = dgram.createSocket({ type: 'udp4', reuseAddr: true });
      sockets.push(sock);
      sock.on('error', (err) => console.warn(`[${iface.name}] socket error: ${err.message}`));
      sock.on('message', (buf, rinfo) => {
        const h = parseSsdp(buf.toString('utf8'));
        if (h.location && !found.has(h.location)) found.set(h.location, { location: h.location, usn: h.usn, server: h.server, from: rinfo.address, via: iface.address });
      });
      sock.bind(0, iface.address, () => {
        sock.setMulticastTTL(2);
        for (let i = 0; i < 3; i++) setTimeout(() => sock.send(msg, SSDP_PORT, SSDP_ADDR), i * 300);
      });
    }
    setTimeout(() => {
      for (const s of sockets) s.close();
      resolve([...found.values()]);
    }, waitMs);
  });
}

async function fetchText(url, init) {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), HTTP_TIMEOUT_MS);
  try {
    const res = await fetch(url, { ...init, signal: ctrl.signal });
    return { status: res.status, text: await res.text() };
  } finally {
    clearTimeout(timer);
  }
}

const tag = (xml, name) => xml.match(new RegExp(`<(?:\\w+:)?${name}[^>]*>([\\s\\S]*?)</(?:\\w+:)?${name}>`))?.[1]?.trim();

function services(xml, base) {
  return [...xml.matchAll(/<(?:\w+:)?service>([\s\S]*?)<\/(?:\w+:)?service>/g)].map((m) => ({
    type: tag(m[1], 'serviceType'),
    control: tag(m[1], 'controlURL') && new URL(tag(m[1], 'controlURL'), base).href,
    scpd: tag(m[1], 'SCPDURL') && new URL(tag(m[1], 'SCPDURL'), base).href,
    events: tag(m[1], 'eventSubURL') && new URL(tag(m[1], 'eventSubURL'), base).href,
  }));
}

async function soap(controlUrl, serviceType, action) {
  const body = `<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:${action} xmlns:u="${serviceType}"></u:${action}></s:Body></s:Envelope>`;
  return fetchText(controlUrl, {
    method: 'POST',
    headers: { 'Content-Type': 'text/xml; charset="utf-8"', SOAPACTION: `"${serviceType}#${action}"` },
    body,
  });
}

const safeName = (s) => s.replace(/[^a-z0-9._-]+/gi, '_').slice(0, 60);

async function describe(hit) {
  const { text: deviceXml } = await fetchText(hit.location);
  const base = tag(deviceXml, 'URLBase') || hit.location;
  const svc = services(deviceXml, base);
  const avt = svc.find((s) => s.type?.includes(':AVTransport:'));
  const cm = svc.find((s) => s.type?.includes(':ConnectionManager:'));
  const rc = svc.find((s) => s.type?.includes(':RenderingControl:'));
  const avtScpd = avt?.scpd ? (await fetchText(avt.scpd).catch(() => ({ text: '' }))).text : '';
  const protocolInfo = cm?.control ? (await soap(cm.control, cm.type, 'GetProtocolInfo').catch((e) => ({ text: String(e) }))).text : '';
  const sink = (tag(protocolInfo, 'Sink') ?? '').split(',').filter(Boolean);
  const info = {
    friendlyName: tag(deviceXml, 'friendlyName'),
    manufacturer: tag(deviceXml, 'manufacturer'),
    modelName: tag(deviceXml, 'modelName'),
    modelNumber: tag(deviceXml, 'modelNumber'),
    udn: tag(deviceXml, 'UDN'),
    location: hit.location,
    server: hit.server,
    reachedVia: hit.via,
    services: svc.map((s) => s.type),
    hasAVTransport: !!avt,
    hasRenderingControl: !!rc,
    supportsSetNext: /SetNextAVTransportURI/.test(avtScpd),
    sinkMimes: [...new Set(sink.map((p) => p.split(':')[2]).filter(Boolean))].sort(),
  };
  if (saveDir) {
    const dir = path.join(saveDir, safeName(`${info.manufacturer ?? 'unknown'}-${info.modelName ?? 'device'}`));
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'device.xml'), deviceXml);
    if (avtScpd) fs.writeFileSync(path.join(dir, 'avtransport-scpd.xml'), avtScpd);
    if (protocolInfo) fs.writeFileSync(path.join(dir, 'get-protocol-info.xml'), protocolInfo);
    fs.writeFileSync(path.join(dir, 'summary.json'), JSON.stringify(info, null, 2));
    info.savedTo = dir;
  }
  return info;
}

const hits = await search();
if (!hits.length) {
  console.log(`No MediaRenderers answered within ${waitMs} ms on: ${lanAddresses().map((a) => `${a.name} ${a.address}`).join(', ') || 'no LAN interfaces'}`);
  process.exit(0);
}
for (const hit of hits) {
  try {
    console.log(JSON.stringify(await describe(hit), null, 2));
  } catch (err) {
    console.warn(`${hit.location}: ${err.message}`);
  }
}
