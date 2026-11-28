#!/usr/bin/env node
// 가짜 AI CLI(시험용) — Claude Code · Codex · OpenCode 의 인자 · 출력 꼴을 흉내 낸다. 모델은 부르지 않는다.
// FAKE_ENGINE: claude-code · codex · opencode, FAKE_AI_OLD=1 이면 옛 판인 척, FAKE_AI_LOG: 받은 인자 · 결과를 적을 파일, FAKE_PAGE_URL: 열 원문 주소
import { readFileSync, appendFileSync } from 'node:fs'
const argv = process.argv.slice(2)
const env = process.env
const engine = env.FAKE_ENGINE || 'claude-code'
const old = env.FAKE_AI_OLD === '1'
const note = (o) => { if (env.FAKE_AI_LOG) appendFileSync(env.FAKE_AI_LOG, JSON.stringify(o) + '\n') }

if (argv[0] === '--version') {
  const v = { 'claude-code': old ? '2.1.200 (Claude Code)' : '2.1.332 (Claude Code)', codex: old ? 'codex-cli 0.150.0' : 'codex-cli 0.172.0', opencode: old ? '1.9.0' : '2.4.2' }[engine]
  console.log(v); process.exit(0)
}
if (argv.includes('--help')) {
  const h = { 'claude-code': '--restricted --tools --settings --strict-mcp-config --permission-prompts --mcp-config', codex: '--ignore-user-config --json --permission-profile', opencode: '--standalone --format' }[engine]
  console.log(h); process.exit(0)
}

const prompt = readFileSync(0, 'utf8')
const val = (flag) => { const i = argv.indexOf(flag); return i >= 0 ? argv[i + 1] : undefined }
const cs = argv.flatMap((a, i) => (a === '-c' ? [argv[i + 1]] : []))
const cval = (k) => { const x = cs.find((c) => c.startsWith(k + '=')); return x ? JSON.parse(x.slice(k.length + 1)) : undefined }
let pes, token, local, ocfg = null
if (engine === 'claude-code') {
  const cfg = JSON.parse(readFileSync(val('--mcp-config'), 'utf8')).mcpServers
  pes = cfg.pes.url; token = cfg.pes.headers.Authorization.replace(/^Bearer /, ''); local = cfg.pes_local.url
} else if (engine === 'codex') {
  pes = cval('mcp_servers.pes.url'); token = env.PES_MCP_TOKEN; local = cval('mcp_servers.pes_local.url')
} else {
  ocfg = JSON.parse(readFileSync(env.OPENCODE_CONFIG, 'utf8'))
  pes = ocfg.mcp.pes.url; token = env.PES_MCP_TOKEN; local = ocfg.mcp.pes_local.url
}
note({ step: 'run', engine, argv, cwd: process.cwd(), prompt_has_pes_local: prompt.includes('pes_local'), opencode_config: ocfg,
  env: { OPENCODE_DISABLE_PROJECT_CONFIG: env.OPENCODE_DISABLE_PROJECT_CONFIG ?? null, LAW_OC: env.LAW_OC ?? null, KOSIS_API_KEY: env.KOSIS_API_KEY ?? null, PES_MCP_TOKEN: !!env.PES_MCP_TOKEN } })

const out = (o) => process.stdout.write(JSON.stringify(o) + '\n')
const toolEvent = (name, command) => {
  if (engine === 'claude-code') out({ type: 'assistant', message: { content: [{ type: 'tool_use', name, input: command ? { command } : {} }] } })
  else if (engine === 'codex') out(command ? { type: 'item.started', item: { type: 'command_execution', command: `/bin/zsh -lc '${command}'` } } : { type: 'item.started', item: { type: 'mcp_tool_call', server: name.split('__')[1], tool: name.split('__')[2] } })
  else out({ type: 'tool_use', part: { tool: command ? 'bash' : name.replace(/^mcp__/, '').replace('__', '_'), state: { status: 'completed', input: command ? { command } : {} } } })
}
let id = 1
async function rpc(url, method, params, auth) {
  const r = await fetch(url, { method: 'POST', headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream', ...(auth ? { authorization: `Bearer ${auth}` } : {}) }, body: JSON.stringify({ jsonrpc: '2.0', id: id++, method, params }) })
  const j = await r.json()
  if (j.error) throw new Error(j.error.message)
  return j.result
}
const call = async (url, name, args, auth) => {
  toolEvent(`mcp__${url === local ? 'pes_local' : 'pes'}__${name}`)
  const r = await rpc(url, 'tools/call', { name, arguments: args }, auth)
  const text = r.content?.[0]?.text ?? ''
  return { isError: !!r.isError, text, data: r.structuredContent ?? (() => { try { return JSON.parse(text) } catch { return null } })() }
}

try {
  out(engine === 'claude-code' ? { type: 'system', subtype: 'init' } : engine === 'codex' ? { type: 'turn.started' } : { type: 'step_start' })
  await rpc(pes, 'initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'fake-ai' } }, token)
  const work = (await call(pes, 'pending_work', {}, token)).data
  const it = work.new[0]
  const { slug, stage } = it
  await call(pes, 'start_round', { slug, stage }, token)
  await rpc(local, 'initialize', { protocolVersion: '2025-06-18', capabilities: {} })
  const tools = (await rpc(local, 'tools/list', {})).tools.map((t) => t.name)
  const kosis = await call(local, 'kosis', { slug, stage, args: ['data', '--tbl', 'DT_TEST'] })
  let fetched = await call(local, 'fetch_url', { slug, stage, url: env.FAKE_PAGE_URL })
  let pageText = fetched.data?.text ?? '', pages = 1
  while (!fetched.isError && fetched.data?.next_from != null) {
    fetched = await call(local, 'read_more', { evidence_id: fetched.data.evidence_id, from: fetched.data.next_from })
    pageText += fetched.data?.text ?? ''; pages++
  }
  const pdf = await call(local, 'fetch_url', { slug, stage, url: env.FAKE_PAGE_URL.replace('page.html', 'att.pdf') })
  const doc = pdf.isError ? pdf : await call(local, 'kordoc', { slug, stage, file: pdf.data.file })
  const ego = await call(local, 'ego_browser', { slug, stage, url: env.FAKE_PAGE_URL.replace('page.html', 'app') })
  const foreign = await call(local, 'kosis', { slug: 'someone-else', stage, args: ['data'] })
  // 감지 시험: 허용 밖 셸 명령(curl)과 허용한 보조 명령(jq)을 쓴 척한다(실제로 돌리지는 않는다)
  toolEvent('Bash', 'curl -s https://example.com')
  toolEvent('Bash', 'jq . a.json')
  const ids = [kosis, fetched].filter((r) => !r.isError).map((r) => r.data.evidence_id)
  const kid = kosis.isError ? null : kosis.data.evidence_id
  const fid = fetched.isError ? null : fetched.data.evidence_id
  const details = `인구는 51,751,065명이다${kid ? `[^${kid}]` : '[^unverified]'}. 기관 원문을 확인했다${fid ? `[^${fid}]` : '[^unverified]'}.`
  await call(pes, 'merge_items', { slug, stage, items: [{ kind: 'finding', title: '인구 현황', details, sources: [] }] }, token)
  await call(pes, 'submit_round', { slug, stage, summary: '시험 라운드', review: { summary: '끝', questions: [] } }, token)
  note({ step: 'results', tools, kosis, fetch_pages: pages, fetch_chars: pageText.length, fetch_tail: pageText.slice(-200), fetch_error: fetched.isError ? fetched.text : null, foreign, pdf, doc, ego, ids })
  if (engine === 'claude-code') out({ type: 'result', subtype: 'success', result: '시험 라운드를 마쳤어요' })
  else if (engine === 'codex') out({ type: 'item.completed', item: { type: 'agent_message', text: '시험 라운드를 마쳤어요' } })
  else out({ type: 'text', part: { text: '시험 라운드를 마쳤어요' } })
} catch (e) {
  note({ step: 'error', error: String(e?.stack ?? e) })
  if (engine === 'claude-code') out({ type: 'result', subtype: 'error', is_error: true, result: String(e) })
  process.exit(1)
}
