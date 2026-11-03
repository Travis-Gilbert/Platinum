// The Code check's ide-cm peer (src/browser/code_check.rs): drives the ide-cm
// page in headless Chrome through puppeteer-core with real keys and clicks, and
// answers one JSON request per stdin line with one JSON reply per stdout line.
//
// FRAME_CODE_PEER_PUPPETEER is the puppeteer-core module directory and
// FRAME_CODE_PEER_CHROME the chrome-headless-shell executable.
//
// Document typing goes one key at a time with a short pause and no wait for
// the session between keys, as a person types: edits buffer while one is in
// flight. ide-cm carries the SDK fix for edits stranded in that buffer (its
// patches/@theoremharness%2Fsdk@...patch).
'use strict'
const puppeteer = require(process.env.FRAME_CODE_PEER_PUPPETEER)
const readline = require('node:readline')

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
let browser
let page
const errors = []
let terminalSession = null

async function state() {
  return page.evaluate(() => {
    const q = (id) => document.querySelector(`[data-testid=${id}]`)
    const lines = [...document.querySelectorAll('.cm-content .cm-line')]
    // Each banner names its path and the revision its conflict was raised at.
    const banners = [...document.querySelectorAll('[data-testid=conflict]')].map((c) => ({
      path: c.dataset.path,
      revision: Number(/\(revision (\d+)\)/.exec(c.textContent)?.[1] ?? NaN),
    }))
    const revision = q('session-revision')?.textContent
    return {
      text: lines.map((line) => line.textContent).join('\n'),
      revision: revision ? Number(revision.slice(1)) : null,
      pending: q('session-sync')?.dataset.pending === 'true',
      dirty: q('session-dirty')?.dataset.dirty === 'true',
      live: q('session-status')?.dataset.live ?? null,
      banners,
      received: window.__codeCheckConflicts ?? [],
    }
  })
}

// Records every `document.conflict` the page's live stream delivers, before
// the SDK hands it to the document session: the SDK reads the stream from
// `response.body`, so the hook gives it one branch of a tee and parses the
// other. The page's own sources are not changed.
function recordConflicts() {
  window.__codeCheckConflicts = []
  const original = window.fetch
  window.fetch = async (...args) => {
    const response = await original(...args)
    const url = String(args[0]?.url ?? args[0])
    if (!/\/api\/environment\/v1\/workspaces\/[^/]+\/live/.test(url) || !response.body) return response
    const [mine, theirs] = response.body.tee()
    Object.defineProperty(response, 'body', { value: theirs })
    void (async () => {
      const reader = mine.pipeThrough(new TextDecoderStream()).getReader()
      let buffer = ''
      for (;;) {
        const { value, done } = await reader.read()
        if (done) return
        buffer += value.replace(/\r\n/g, '\n')
        let end
        while ((end = buffer.indexOf('\n\n')) >= 0) {
          const block = buffer.slice(0, end).split('\n')
          buffer = buffer.slice(end + 2)
          const event = block.find((line) => line.startsWith('event:'))?.slice(6).trim()
          const data = block.filter((line) => line.startsWith('data:')).map((line) => line.slice(5).trimStart()).join('\n')
          let parsed = null
          try {
            parsed = JSON.parse(data)
          } catch {}
          // The host's frame repeats its name and cursor inside `data`.
          if ((event ?? parsed?.kind) === 'document.conflict' && parsed?.data) {
            window.__codeCheckConflicts.push({
              cursor: parsed.cursor,
              document: parsed.data.document_id,
              revision: parsed.data.revision,
            })
          }
        }
      }
    })().catch(() => {})
    return response
  }
}

async function settle(limit = 10000) {
  const deadline = Date.now() + limit
  for (;;) {
    const now = await state()
    if (!now.pending || Date.now() > deadline) return now
    await sleep(25)
  }
}

async function operation(name, args) {
  return page.evaluate(
    async (name, args, workspace) => {
      const boot = await (await fetch('/api/environment/v1/bootstrap')).json()
      const response = await fetch(`/api/environment/v1/workspaces/${workspace}/operations/${name}`, {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          'X-CSRF-Token': boot.csrfToken,
          'Idempotency-Key': crypto.randomUUID(),
        },
        body: JSON.stringify(args),
      })
      return { status: response.status, body: await response.json().catch(() => null) }
    },
    name,
    args,
    new URL(page.url()).searchParams.get('workspace'),
  )
}

async function terminalText() {
  return page.evaluate(() => [...document.querySelectorAll('.xterm-rows')].map((rows) => rows.innerText).join('\n'))
}

const handlers = {
  async open({ url }) {
    browser = await puppeteer.launch({
      executablePath: process.env.FRAME_CODE_PEER_CHROME,
      headless: 'shell',
      defaultViewport: { width: 1400, height: 900 },
    })
    page = await browser.newPage()
    await page.evaluateOnNewDocument(recordConflicts)
    page.on('pageerror', (error) => errors.push(String(error).slice(0, 300)))
    page.on('response', async (response) => {
      if (!response.url().endsWith('/operations/terminal.open')) return
      const body = await response.json().catch(() => null)
      if (body && body.session) terminalSession = body.session
    })
    page.on('console', (message) => {
      if (message.type() === 'error') errors.push(message.text().slice(0, 300))
    })
    // The dev server may still be starting.
    const deadline = Date.now() + 60000
    for (;;) {
      try {
        await page.goto(url, { waitUntil: 'networkidle2', timeout: 30000 })
        await page.waitForSelector('[data-testid=session-revision]', { timeout: 30000 })
        break
      } catch (error) {
        if (Date.now() > deadline) throw error
        await sleep(500)
      }
    }
    return settle()
  },
  state,
  async type({ at, text }) {
    await page.click('.cm-content')
    await page.keyboard.down('Meta')
    await page.keyboard.press(at === 'start' ? 'Home' : 'ArrowDown')
    await page.keyboard.up('Meta')
    for (const key of text) {
      await page.keyboard.type(key)
      await sleep(20)
    }
    return settle()
  },
  async undo() {
    await page.click('.cm-content')
    await page.keyboard.down('Meta')
    await page.keyboard.press('z')
    await page.keyboard.up('Meta')
    await sleep(50)
    return settle()
  },
  async keep({ side }) {
    const label = side === 'disk' ? 'Use the checkout' : 'Keep my edits'
    const [button] = await page.$$(`xpath/.//*[@data-testid="conflict"]//button[normalize-space()="${label}"]`)
    if (!button) throw new Error(`no "${label}" button`)
    await button.click()
    return settle()
  },
  async call({ operation: name, arguments: args }) {
    return operation(name, args)
  },
  async 'terminal.open'() {
    await page.click('button[aria-label="Terminal"]')
    await page.waitForSelector('.xterm-helper-textarea', { timeout: 30000 })
    await sleep(500)
    return { session: terminalSession, text: await terminalText() }
  },
  async 'terminal.type'({ text }) {
    await page.focus('.xterm-helper-textarea')
    await page.keyboard.type(text, { delay: 20 })
    return { text: await terminalText() }
  },
  async 'terminal.text'() {
    return { text: await terminalText() }
  },
  async shot({ path }) {
    await page.screenshot({ path })
    return { path }
  },
  async errors() {
    return { errors }
  },
  async close() {
    await browser?.close()
    return {}
  },
}

const lines = readline.createInterface({ input: process.stdin })
let queue = Promise.resolve()
lines.on('line', (line) => {
  queue = queue.then(async () => {
    let reply
    try {
      const request = JSON.parse(line)
      reply = { ok: await handlers[request.op](request) }
    } catch (error) {
      reply = { error: String(error && error.stack ? error.stack : error).slice(0, 2000) }
    }
    process.stdout.write(JSON.stringify(reply) + '\n')
  })
})
lines.on('close', () => {
  queue.then(() => browser?.close()).finally(() => process.exit(0))
})
