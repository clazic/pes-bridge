// PES 브리지의 키 금고(Vault) · 로컬 중계(Relay) — 규약은 저장소의 RELAY.md.
//
// 외부 AI가 돌 때 조사 도구 CLI(kosis · sgis · vworld · korean-law)는 API 키를 받지 않는다.
//   1) Vault: PES 서버에 등록된 키를 받아(POST /mcp/tool-keys) 이 프로세스의 메모리에만 둔다. 파일 · 환경 변수 · 로그에 쓰지 않는다.
//      · 계정 폴더의 기기 키(ECDSA P-256, device.json 0600)로 서명한 요청만 서버가 받는다 — 연결 토큰은 AI 프로세스도 갖고 있어서(MCP 연결) 토큰만으로는 받지 못한다.
//      · 요청마다 일회용 키(ECDH P-256)를 만들어 보내고, 서버는 그 키로만 풀리는 AES-256-GCM 암호문을 돌려준다(HTTPS 를 가로채거나 기록에 남아도 키가 보이지 않는다).
//   2) Relay: 127.0.0.1 에 연 중계. CLI 는 기관 서버 대신 여기로 자리표시(__PES_KEY_0__ …)를 넣어 보내고, 중계가 실제 키로 바꿔 기관 서버(HTTPS)로 보낸다.
//      · 실행마다 새 토큰(open → close) — 실행이 끝나면 그 토큰은 죽는다. AI 가 토큰을 봐도 그 실행 동안 기관 API 를 부를 수 있을 뿐 키는 얻지 못한다.
//      · 도구마다 정해진 기관 호스트로만 보낸다(다른 곳으로 키가 실린 요청을 보내게 할 수 없다). 응답에 실제 키가 섞여 오면 자리표시로 바꾼다.
//      · 키마다 호출 수를 센다(usage) — 실행이 끝날 때 브리지가 로그에 남긴다.
import http from 'node:http'
import https from 'node:https'
import { randomBytes, timingSafeEqual, webcrypto } from 'node:crypto'
import { chmodSync, existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const { subtle } = webcrypto
const te = new TextEncoder(), td = new TextDecoder()
const b64 = (u) => Buffer.from(u).toString('base64')
const unb64 = (s) => new Uint8Array(Buffer.from(String(s), 'base64'))
const P256 = { name: 'ECDSA', namedCurve: 'P-256' }

/** 서버가 내주는 도구 이름 → 중계 경로의 도구 이름 · 환경 변수 이름 */
export const RELAY_TOOLS = { kosis: 'KOSIS', sgis: 'SGIS', vworld: 'VWORLD', law: 'LAW' }
const KO = { kosis: 'KOSIS', sgis: 'SGIS', vworld: 'VWorld', law: '법령' } // 안내 글에 쓰는 이름(CLI 의 안내와 같게)
/** 도구마다 보낼 수 있는 기관 호스트(그 이름이거나 그 아래 도메인) — 다른 곳으로는 보내지 않는다 */
const HOSTS = {
  kosis: ['kosis.kr'],
  sgis: ['sgisapi.kostat.go.kr', 'sgis.kostat.go.kr', 'sgisapi.mods.go.kr', 'sgis.mods.go.kr'],
  vworld: ['vworld.kr'],
  law: ['law.go.kr', 'nts.go.kr'], // nts: 국세법령정보시스템(taxlaw.nts.go.kr) — 키 없이 부르는 곳
}
const hostOk = (tool, host) => { const h = String(host).toLowerCase().replace(/:\d+$/, ''); return (HOSTS[tool] ?? []).some((d) => h === d || h.endsWith(`.${d}`)) }

// ───────── Vault: 계정 하나의 키 ─────────
export class Vault {
  /** base: PES 주소 · dir: 계정 폴더 · token(): 지금 연결 토큰 · log */
  constructor({ base, dir, token, log }) { this.base = base; this.file = join(dir, 'device.json'); this.token = token; this.log = log ?? (() => {}); this.dev = null; this.bound = null; this.cache = null; this.off = 0 }

  /** 기기 키 — 없으면 만든다(개인 키는 이 파일에만, 0600) */
  async device() {
    if (this.dev) return this.dev
    let j = null
    try { j = JSON.parse(readFileSync(this.file, 'utf8')) } catch {}
    if (!j?.jwk || !j?.pub) {
      const kp = await subtle.generateKey(P256, true, ['sign', 'verify'])
      j = { jwk: await subtle.exportKey('jwk', kp.privateKey), pub: b64(await subtle.exportKey('raw', kp.publicKey)) }
      writeFileSync(this.file, JSON.stringify(j), { mode: 0o600 })
      try { chmodSync(this.file, 0o600) } catch {}
    }
    this.dev = { key: await subtle.importKey('jwk', j.jwk, P256, false, ['sign']), pub: j.pub }
    return this.dev
  }
  async sign(msg) { const d = await this.device(); return b64(await subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, d.key, te.encode(msg))) }
  async post(path, body, token) {
    const r = await fetch(`${this.base}${path}`, { method: 'POST', headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` }, body: JSON.stringify(body) }).catch(() => null)
    if (!r) return { status: 0, data: null }
    return { status: r.status, data: await r.json().catch(() => null) }
  }
  /** 이 연결에 기기 키를 묶는다(연결마다 한 번 — 토큰이 바뀌면 다시 확인) */
  async bind(token) {
    if (this.bound === token) return true
    const d = await this.device(), ts = Date.now()
    const r = await this.post('/mcp/device', { pub: d.pub, ts, sig: await this.sign(`pes-device\n${ts}\n${d.pub}`) }, token)
    if (r.status === 200) { this.bound = token; return true }
    if (r.status === 404 || r.status === 405) return null // 예전 서버 — 키 내주기가 없다
    this.log(`조사 도구 키: 기기 키를 묶지 못했어요 — ${r.data?.error ?? `HTTP ${r.status}`}`)
    return false
  }
  /**
   * 서버에 등록된 키를 받는다 → { kosis: {keys:[{key,secret?}], extra?}, … } (없으면 null — 예전 서버 · 연결 문제)
   * 5분 동안은 메모리의 것을 다시 쓴다. 못 받으면 1분 쉰다(서버를 두드리지 않게)
   */
  async keys(force = false) {
    const now = Date.now()
    if (!force && this.cache && now - this.cache.at < 300_000) return this.cache.tools
    if (!force && now < this.off) return this.cache?.tools ?? null
    try {
      const token = await this.token()
      if (!token) return null
      const ok = await this.bind(token)
      if (!ok) { this.off = now + (ok === null ? 3_600_000 : 60_000); return null }
      const eph = await subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, false, ['deriveBits'])
      const epk = b64(await subtle.exportKey('raw', eph.publicKey)), nonce = randomBytes(24).toString('base64url'), ts = Date.now()
      const r = await this.post('/mcp/tool-keys', { ts, nonce, epk, sig: await this.sign(`pes-tool-keys\n${ts}\n${nonce}\n${epk}`) }, token)
      if (r.status !== 200 || !r.data?.ct) {
        if (r.status !== 404) this.log(`조사 도구 키를 받지 못했어요 — ${r.data?.error ?? `HTTP ${r.status}`}`)
        this.off = now + (r.status === 404 ? 3_600_000 : 60_000); return null
      }
      const peer = await subtle.importKey('raw', unb64(r.data.spk), { name: 'ECDH', namedCurve: 'P-256' }, false, [])
      const shared = await subtle.deriveBits({ name: 'ECDH', public: peer }, eph.privateKey, 256)
      const hk = await subtle.importKey('raw', shared, 'HKDF', false, ['deriveKey'])
      const aes = await subtle.deriveKey({ name: 'HKDF', hash: 'SHA-256', salt: te.encode(nonce), info: te.encode('pes-tool-keys v1') }, hk, { name: 'AES-GCM', length: 256 }, false, ['decrypt'])
      const plain = await subtle.decrypt({ name: 'AES-GCM', iv: unb64(r.data.iv), additionalData: te.encode(`${ts}.${nonce}`) }, aes, unb64(r.data.ct))
      const tools = JSON.parse(td.decode(plain)).tools ?? {}
      this.cache = { at: Date.now(), tools }
      return tools
    } catch (e) { this.log(`조사 도구 키 받기 오류 — ${String(e?.message ?? e).slice(0, 120)}`); this.off = now + 60_000; return null }
  }
  /** 로컬 중계가 센 키별 호출 수를 서버에 올린다(「조사 도구 · 그림 키」 창의 사용량) — 기기 키로 서명. 실패해도 일에는 지장 없다 */
  async report(usage) {
    try {
      const list = {}
      for (const [t, a] of Object.entries(usage ?? {})) { const x = a.filter((c) => c?.id && c.calls); if (x.length) list[t] = x.map(({ id, calls, errors, limited }) => ({ id, calls, errors, limited })) }
      if (!Object.keys(list).length) return false
      const token = await this.token()
      if (!token || !(await this.bind(token))) return false
      const u = JSON.stringify(list), nonce = randomBytes(24).toString('base64url'), ts = Date.now()
      const r = await this.post('/mcp/tool-usage', { ts, nonce, u, sig: await this.sign(`pes-tool-usage\n${ts}\n${nonce}\n${u}`) }, token)
      return r.status === 200
    } catch { return false }
  }
  forget() { this.cache = null }
}

// ───────── Relay: 로컬 중계 ─────────
const HOP = new Set(['connection', 'keep-alive', 'proxy-authenticate', 'proxy-authorization', 'te', 'trailer', 'transfer-encoding', 'upgrade', 'host', 'content-length', 'accept-encoding', 'x-pes-relay-token'])
const PH = /__PES_(KEY|SECRET)_(\d+)__|__PES_DOMAIN__/g
const TEXTY = /^(text\/|application\/(json|xml|javascript|x-www-form-urlencoded|[\w.+-]*\+(json|xml)))/i
const MAX_BODY = 8 << 20 // 자리표시를 바꾸려고 통째로 읽는 요청 본문 한도
const same = (a, b) => { const x = Buffer.from(String(a)), y = Buffer.from(String(b)); return x.length === y.length && timingSafeEqual(x, y) }

export class Relay {
  constructor({ log } = {}) { this.log = log ?? (() => {}); this.runs = new Map(); this.server = null; this.port = 0 }

  listen() {
    if (this.server) return Promise.resolve(this.port)
    return new Promise((res, rej) => {
      const s = http.createServer((q, r) => { this.handle(q, r).catch((e) => this.fail(r, 502, `중계 오류: ${String(e?.message ?? e).slice(0, 120)}`)) })
      s.on('error', rej)
      s.listen(0, '127.0.0.1', () => { this.server = s; this.port = s.address().port; s.unref(); res(this.port) })
    })
  }
  /** 실행 하나를 연다 — tools: Vault.keys() 결과. 돌려주는 env 를 AI 프로세스에 준다(키는 없다) */
  async open(tools) {
    await this.listen()
    const token = randomBytes(32).toString('base64url')
    const pool = {}
    for (const t of Object.keys(RELAY_TOOLS)) pool[t] = { keys: (tools?.[t]?.keys ?? []).filter((k) => k?.key), domain: String(tools?.[t]?.extra ?? '') }
    this.runs.set(token, { pool, usage: {} })
    const env = { PES_RELAY_URL: `http://127.0.0.1:${this.port}`, PES_RELAY_TOKEN: token }
    for (const [t, E] of Object.entries(RELAY_TOOLS)) env[`PES_RELAY_${E}_KEYS`] = String(pool[t].keys.length)
    return { token, env }
  }
  /** 실행을 닫는다 → 그 실행의 키별 호출 수 {도구: [{id(서버의 키 번호), calls, errors, limited}]} — 자리(키 차례)마다, 안 쓴 자리는 빈칸 */
  close(token) {
    const r = this.runs.get(token); this.runs.delete(token)
    const out = {}
    for (const [t, a] of Object.entries(r?.usage ?? {})) out[t] = Array.from(a, (c, i) => (c ? { id: r.pool[t].keys[i]?.id ?? 0, ...c } : null))
    return out
  }
  stop() { try { this.server?.close() } catch {} this.server = null; this.runs.clear() }

  fail(res, status, error) {
    if (res.headersSent) { try { res.destroy() } catch {} return }
    const b = Buffer.from(JSON.stringify({ error }))
    res.writeHead(status, { 'content-type': 'application/json; charset=utf-8', 'content-length': b.length, 'x-pes-relay-error': '1', 'cache-control': 'no-store' })
    res.end(b)
  }

  async handle(req, res) {
    // 이 컴퓨터의 CLI 만 — 다른 이름으로 들어온 요청(이름 바꿔치기)은 받지 않는다. 브라우저의 다른 사이트 스크립트는 토큰 머리글을 붙이지 못한다
    // (사전 요청 OPTIONS 에 답하지 않는다). Origin 머리글만으로는 막지 않는다 — CLI 가 기관에 보내는 Origin(국세법령정보)도 있다
    const host = String(req.headers.host ?? '')
    if (host !== `127.0.0.1:${this.port}` || req.method === 'OPTIONS' || req.headers['sec-fetch-site']) return this.fail(res, 403, '이 주소로는 중계하지 않아요.')
    const tok = String(req.headers['x-pes-relay-token'] ?? '')
    let run = null
    for (const [t, r] of this.runs) if (same(t, tok)) run = r
    if (!run) return this.fail(res, 401, '중계 토큰이 맞지 않아요(실행이 끝났을 수 있어요).')
    const m = /^\/([a-z]+)\/(https?)\/([^/?#]+)(\/[^?#]*)?(\?[^#]*)?$/.exec(req.url ?? '')
    if (!m) return this.fail(res, 400, '중계 주소가 맞지 않아요.')
    const [, tool, , upHost, path = '/', query = ''] = m
    if (!RELAY_TOOLS[tool] || !hostOk(tool, upHost)) return this.fail(res, 403, `${tool} 중계로는 ${upHost} 에 보낼 수 없어요.`)
    const pool = run.pool[tool]
    if (!pool.keys.length) return this.fail(res, 409, `PES 에 ${KO[tool]} 키가 없어요. PES 「조사 도구 · 그림 키」에서 키를 넣으세요.`)

    // 자리표시 → 실제 값(어느 키를 썼는지 기억해 센다). enc: 값이 들어가는 자리에 맞게 바꾼다
    const used = new Set()
    let bad = null
    const fill = (s, enc) => String(s).replace(PH, (all, kind, i) => {
      if (all === '__PES_DOMAIN__') return enc(pool.domain)
      const k = pool.keys[Number(i)]
      if (!k) { bad = `${tool} 키 ${i}번이 없어요(등록된 키 ${pool.keys.length}개).`; return all }
      used.add(Number(i))
      return enc(kind === 'KEY' ? k.key : (k.secret ?? ''))
    })
    const raw = (v) => v
    const headers = {}
    for (const [k, v] of Object.entries(req.headers)) {
      if (HOP.has(k.toLowerCase())) continue
      // Referer 에 키 자리표시가 있으면(주소 넘김을 따라갈 때 앞 주소가 실린다) 그 머리글은 보내지 않는다 — 키가 머리글로 나가지 않게. 등록 도메인 자리표시는 바꿔 보낸다
      if (k.toLowerCase() === 'referer' && /__PES_(KEY|SECRET)_\d+__/.test(String(v))) continue
      // 등록 도메인이 없는 키(서버용 키): 도메인 자리표시가 든 머리글은 보내지 않는다
      if (!pool.domain && String(v).includes('__PES_DOMAIN__')) continue
      headers[k] = Array.isArray(v) ? v.map((x) => fill(x, raw)) : fill(v, raw)
    }
    headers['accept-encoding'] = 'identity' // 응답 글에서 키를 지우려고 압축 없이 받는다
    // 등록 도메인이 없으면 domain 인자(자리표시뿐인 것)는 빼고 보낸다. 경로에도 키가 들어갈 수 있다(VWorld 타일 주소)
    const q = pool.domain ? query : query.replace(/([?&])[^=&]+=__PES_DOMAIN__(&|$)/g, (_, a, b) => (b ? a : '')).replace(/[?&]$/, '')
    const target = `${fill(path, encodeURIComponent)}${fill(q, encodeURIComponent)}`

    // 요청 본문: 글이면 통째로 읽어 자리표시를 바꾼다(JSON · form · XML). 그 밖은 그대로 흘려보낸다
    const ctype = String(req.headers['content-type'] ?? '')
    let body = null
    if (req.method !== 'GET' && req.method !== 'HEAD' && TEXTY.test(ctype)) {
      const chunks = []; let n = 0
      for await (const c of req) { n += c.length; if (n > MAX_BODY) return this.fail(res, 413, '요청 본문이 너무 커요.'); chunks.push(c) }
      const form = /x-www-form-urlencoded/i.test(ctype), jsonish = /json/i.test(ctype)
      body = Buffer.from(fill(Buffer.concat(chunks).toString('utf8'), form ? encodeURIComponent : jsonish ? (v) => JSON.stringify(String(v)).slice(1, -1) : raw))
      headers['content-length'] = String(body.length)
    } else if (req.headers['content-length']) headers['content-length'] = req.headers['content-length']
    if (bad) return this.fail(res, 409, bad)

    // 응답에서 지울 것 — 이 도구의 실제 키 · 비밀(그대로 · 주소 꼴). 자리표시로 바꾼다
    const secrets = []
    pool.keys.forEach((k, i) => { for (const [v, ph] of [[k.key, `__PES_KEY_${i}__`], [k.secret, `__PES_SECRET_${i}__`]]) if (v && v.length >= 6) { secrets.push([v, ph]); const e = encodeURIComponent(v); if (e !== v) secrets.push([e, ph]) } })
    const scrub = (s) => { for (const [v, ph] of secrets) if (s.includes(v)) s = s.split(v).join(ph); return s }
    const hold = Math.max(0, ...secrets.map(([v]) => v.length - 1))
    const count = (status) => { const u = (run.usage[tool] ??= []); for (const i of used.size ? used : [0]) { const c = (u[i] ??= { calls: 0, errors: 0, limited: 0 }); c.calls++; if (status >= 400) c.errors++; if (status === 429) c.limited++ } }

    await new Promise((done) => {
      const up = https.request({ host: upHost.replace(/:\d+$/, ''), port: Number(/:(\d+)$/.exec(upHost)?.[1] ?? 443), method: req.method, path: target, headers: { ...headers, host: upHost }, timeout: 600_000 }, (r) => {
        count(r.statusCode ?? 0)
        const out = {}
        for (const [k, v] of Object.entries(r.headers)) { if (HOP.has(k.toLowerCase())) continue; out[k] = Array.isArray(v) ? v.map(scrub) : scrub(String(v)) }
        const texty = TEXTY.test(String(r.headers['content-type'] ?? '')) && !r.headers['content-encoding']
        if (!texty && r.headers['content-length']) out['content-length'] = r.headers['content-length']
        res.writeHead(r.statusCode ?? 502, out)
        if (!texty || !secrets.length) { r.pipe(res); r.on('end', done); r.on('error', () => { res.destroy(); done() }); return }
        // 글 응답: 조각 경계에 걸친 키도 지우게, 끝의 (가장 긴 키 - 1)글자는 다음 조각과 함께 본다. latin1 로 바이트를 그대로 지킨다(키는 ASCII)
        let tail = ''
        r.on('data', (c) => { const s = scrub(tail + c.toString('latin1')); const cut = Math.max(0, s.length - hold); tail = s.slice(cut); if (cut) res.write(Buffer.from(s.slice(0, cut), 'latin1')) })
        r.on('end', () => { res.end(Buffer.from(scrub(tail), 'latin1')); done() })
        r.on('error', () => { res.destroy(); done() })
      })
      up.on('timeout', () => up.destroy(new Error('기관 서버가 응답하지 않아요')))
      up.on('error', (e) => { this.fail(res, 502, `기관 서버에 닿지 않아요: ${String(e?.message ?? e).slice(0, 120)}`); done() })
      if (body) up.end(body); else if (req.method === 'GET' || req.method === 'HEAD') up.end(); else req.pipe(up)
    })
  }
}
