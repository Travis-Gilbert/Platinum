// Writes what web Theo does, as data, for the Rust port to be checked against.
//
// Run with bun from anywhere, given a checkout of Travis-Gilbert/Theorem-UI:
//
//   bun crates/theo/fixtures/generate.ts /path/to/theorem-ui
//
// It imports the web implementation's own modules (src/lib/face) and runs
// them; nothing here restates what they compute. It writes
// `../data/builtin.json` (the book and the rules, which the crate ships) and
// the files beside this one (what the crate's tests compare with).
import { execSync } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'

const root = resolve(process.argv[2] ?? '')
if (!process.argv[2]) throw new Error('Pass the path of a Theorem-UI checkout.')
const face = (name: string) => import(join(root, 'src/lib/face', `${name}.ts`))

const { BUILT_IN } = await face('book')
const { Channels, FACE_CHANNELS, FEATURE_PARAMS } = await face('channels')
const { Face, plainGaze } = await face('engine')
const { restShape, seatPoint } = await face('feature')
const { THEO } = await face('identity')
const { Field } = await face('particles')
const { compile } = await face('program')
const { DEFAULT_RULES } = await face('reactions')
const { SIGNALS } = await face('signals')
const { seeded, seedOf } = await face('random')

const here = dirname(new URL(import.meta.url).pathname)
const commit = execSync('git rev-parse HEAD', { cwd: root }).toString().trim()
const source = { repository: 'Travis-Gilbert/Theorem-UI', commit }
const write = (path: string, value: unknown) => {
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, `${JSON.stringify(value)}\n`)
}

// What the crate ships: the identity, the channel and signal tables, the book
// and the rules. They are data on the web too, so none is restated by hand.
write(join(here, '../data/builtin.json'), {
  source,
  identity: THEO,
  featureParams: FEATURE_PARAMS,
  faceChannels: FACE_CHANNELS,
  signals: SIGNALS,
  book: BUILT_IN,
  rules: DEFAULT_RULES,
})

// The tables the port restates by hand, and the generator.
const draws = (seed: number) => {
  const random = seeded(seed)
  return Array.from({ length: 8 }, () => random())
}
write(join(here, 'specs.json'), {
  source,
  featureParams: FEATURE_PARAMS,
  faceChannels: FACE_CHANNELS,
  signals: SIGNALS,
  identity: THEO,
  random: [1, 7, 20261111, 0xffffffff].map((seed) => ({
    seed,
    draws: draws(seed),
  })),
  seedOf: ['', 'theo', 'eye.left', 'Thé'].map((text) => ({
    text,
    seed: seedOf(text),
  })),
})

// Where grains sit.
const shapes: Array<[string, Record<string, unknown>]> = [
  ['rest', {}],
  ['left', { side: -1 }],
  ['right-ellipse', { side: 1, round: 2, w: 9, h: 15 }],
  ['diamond-hollow', { round: 1, hollow: 0.6 }],
  ['gains', { gains: [1.4, 0.6, 1, 1.2, 0.3, 1.8, 0.9, 1.1] }],
  ['lids', { side: 1, lid: 0.4, lidTilt: 0.5, lidBow: -0.3, low: 0.2 }],
  ['shut', { side: -1, lid: 1, lowTilt: 0.4, lowBow: 0.7 }],
  ['rings', { rings: 3, round: 2, hollow: 0.2 }],
  ['slats', { slats: 5, w: 20, h: 6 }],
  ['mesh-even', { side: -1, mesh: 10, round: 2, w: 14, h: 14 }],
  ['mesh-odd', { side: 1, mesh: 7, round: 4, w: 16, h: 11 }],
  ['globe', { globe: 9, round: 2, w: 13, h: 13 }],
  [
    'globe-turned',
    { globe: 9, round: 2, w: 13, h: 13, spun: 3.37, stirred: 1.9, stir: 0.6 },
  ],
  [
    'globe-loose',
    {
      globe: 7,
      round: 2,
      w: 11,
      h: 11,
      spun: 12.25,
      stirred: 7.1,
      stir: 1,
      loose: 0.8,
    },
  ],
  ['turned', { side: 1, turn: 35, scale: 1.4, x: 12, y: -6 }],
  ['centre-turned', { side: 0, turn: -120, scale: 0.5, lid: 0.7 }],
]
const seats = shapes.map(([name, fields]) => {
  const shape = restShape()
  const { gains, ...rest } = fields as { gains?: Array<number> }
  Object.assign(shape, rest)
  if (gains) shape.gains.set(gains)
  const out = [0, 0]
  const points: Array<Array<number>> = []
  const random = seeded(seedOf(name))
  for (let index = 0; index < 160; index++) {
    // A grid of the rim and the inside, and draws between.
    const theta = index < 40 ? (index / 40) * Math.PI * 2 : random() * Math.PI * 2
    const u = index < 40 ? 1 : index % 5 === 0 ? 1 : random()
    const light = seatPoint(shape, theta, u, out, 0)
    points.push([theta, u, out[0] as number, out[1] as number, light])
  }
  return { name, shape: { ...shape, gains: [...shape.gains] }, points }
})
write(join(here, 'seats.json'), { source, seats })

// The matter's own numbers.
const fields = [7, 20261111].map((seed) => {
  const field = new Field(THEO, seed) as unknown as Record<string, Float32Array>
  return {
    seed,
    theta: [...(field.theta as Float32Array)],
    u: [...(field.u as Float32Array)],
    phase: [...(field.phase as Float32Array)],
  }
})
write(join(here, 'field.json'), { source, fields })

// Programs laid out in time.
const book = new Map(BUILT_IN.map((entry: { name: string }) => [entry.name, entry]))
const lookup = (name: string) =>
  (book.get(name.split('@')[0] ?? '') as { program: unknown } | undefined)?.program
const written: Array<[string, unknown, Record<string, unknown>]> = [
  ['list', [{ op: 'to', set: { 'eyes.lid': 1 }, over: 80 }], {}],
  ['one-step', { op: 'wait', ms: { rand: [100, 900] } }, { seed: 5 }],
  [
    'pulled-in',
    { steps: [{ op: 'to', set: { 'eyes.w': 99, 'face.tilt': -90 } }], keep: true },
    {},
  ],
  [
    'by-and-args',
    {
      steps: [
        { op: 'by', set: { 'face.dy': { arg: 'lift', or: -4 } }, hold: 200 },
        { op: 'to', set: { 'gaze.x': { arg: 'x' } }, over: 0 },
      ],
    },
    { args: { x: 0.5 }, intensity: 1.5 },
  ],
  [
    'feature-and-follow',
    {
      steps: [
        { op: 'feature', id: 'brow.left', set: { y: -18, h: 2, w: 10 } },
        { op: 'feature', id: 'mouth', set: { y: 20, share: 0.6 } },
        {
          op: 'par',
          steps: [
            { op: 'follow', channel: 'mouth.h', signal: 'sound.level', amp: 6 },
            {
              op: 'follow',
              channel: 'eyes.h',
              signal: 'sound.beat',
              amp: { rand: [1, 2] },
              for: 900,
            },
            { op: 'wave', channel: 'brow.left.y', amp: 2, period: 300, for: 1200, shape: 'noise' },
          ],
        },
        { op: 'release', channels: ['mouth.h'], over: 100 },
        { op: 'feature', id: 'brow.left', remove: true },
      ],
    },
    { seed: 99 },
  ],
  [
    'composed',
    {
      steps: [
        {
          op: 'stagger',
          gap: 120,
          steps: [
            { op: 'look', at: 'composer', hold: 100 },
            { op: 'look', at: { x: 3, y: -0.5 } },
            { op: 'go', to: { x: 0.2, y: 4 }, size: 9 },
          ],
        },
        {
          op: 'repeat',
          times: 3,
          step: {
            op: 'vary',
            of: [
              { op: 'use', name: 'blink' },
              { op: 'use', name: 'nod', intensity: 0.5 },
              { op: 'burst', x: 80, strength: { rand: [50, 900] } },
            ],
            weights: [2, 1, 1],
          },
        },
      ],
    },
    { seed: 31337 },
  ],
  [
    'thing',
    { steps: [{ op: 'go', to: { thing: 'tool:last', text: '  export  ', line: 3 } }] },
    {},
  ],
  // Refused.
  ['not-a-program', 'blink', {}],
  ['no-op', { steps: [{ set: {} }] }, {}],
  ['unknown-op', { steps: [{ op: 'jump' }] }, {}],
  ['unknown-channel', { steps: [{ op: 'to', set: { 'eyes.lib': 1 } }] }, {}],
  ['far-channel', { steps: [{ op: 'to', set: { zzzzzzzzzzzzzzzzzzzz: 1 } }] }, {}],
  ['missing-arg', { steps: [{ op: 'wait', ms: { arg: 'gone' } }] }, {}],
  ['bad-wave', { steps: [{ op: 'wave', channel: 'face.dx', amp: 1, period: 10 }] }, {}],
  ['bad-signal', { steps: [{ op: 'follow', channel: 'face.dx', signal: 'mic.level', amp: 1 }] }, {}],
  ['bad-feature', { steps: [{ op: 'feature', id: 'face.brow' }] }, {}],
  ['bad-id', { steps: [{ op: 'par', steps: [{ op: 'feature', id: 'Brow' }] }] }, {}],
  ['no-feature', { steps: [{ op: 'feature', id: 'brow', remove: true }] }, {}],
  ['too-long', { steps: [{ op: 'repeat', times: 24, step: { op: 'wait', ms: 2000 } }] }, {}],
  ['bad-times', { steps: [{ op: 'repeat', times: 25, step: { op: 'wait', ms: 1 } }] }, {}],
  ['bad-place', { steps: [{ op: 'look', at: 'ceiling' }] }, {}],
  ['bad-thing', { steps: [{ op: 'go', to: { thing: 'Tool' } }] }, {}],
  ['no-book', { steps: [{ op: 'use', name: 'moonwalk' }] }, {}],
  ['bad-release', { steps: [{ op: 'release', channels: ['eyes.lid', 3] }] }, {}],
  ['bad-time', { steps: [{ op: 'to', set: {}, over: -1 }] }, {}],
]
const context = (extra: Record<string, unknown>) => ({
  channels: new Channels(THEO.features),
  lookup,
  ...extra,
})
const scores = [
  ...BUILT_IN.flatMap((entry: { name: string; program: unknown }) =>
    [
      { seed: 1 },
      { seed: 20261111, intensity: 0.6 },
      { seed: 4242, intensity: 1.6 },
    ].map((extra) => ({
      name: entry.name,
      input: entry.program,
      context: extra,
      checked: compile(entry.program, context(extra)),
    })),
  ),
  ...written.map(([name, input, extra]) => ({
    name,
    input,
    context: extra,
    checked: compile(input, context(extra)),
  })),
]
write(join(here, 'scores.json'), { source, scores })

// The whole face, on a clock the script turns.
type Act =
  | { act: 'run'; ms: number }
  | { act: 'notice'; kind: string; weight?: number }
  | { act: 'liveness'; to: string }
  | { act: 'feed'; signals: Record<string, number> }
  | { act: 'still'; on: boolean }
  | { act: 'poke'; x: number; y: number }
  | { act: 'perform'; input: unknown; how: Record<string, unknown> }
  | { act: 'pointer'; gaze?: { x: number; y: number } }
  | { act: 'watch'; gaze?: { x: number; y: number } }
  | { act: 'heading'; gaze?: { x: number; y: number } }
  | { act: 'grab' }
  | { act: 'carry'; dx: number; dy: number }
  | { act: 'drop' }
  | { act: 'toss'; force: number }
  | { act: 'sway'; dx: number; dy: number; seconds: number }
  | { act: 'bump'; side: string; force: number; against?: string }
  | { act: 'express'; expression?: Record<string, unknown> }
  | { act: 'tint'; hue: number }
  | { act: 'unform' }

const sound = (level: number, beat = 0) => ({
  'sound.level': level,
  'sound.low': level * 0.8,
  'sound.mid': level * 0.6,
  'sound.high': level * 0.3,
  'sound.beat': beat,
})
const quiet = sound(0)
const scenarios: Array<{ name: string; seed: number; acts: Array<Act> }> = [
  {
    name: 'rest-and-touch',
    seed: 7,
    acts: [
      { act: 'run', ms: 400 },
      { act: 'pointer', gaze: { x: 0.8, y: -0.4 } },
      { act: 'run', ms: 600 },
      { act: 'poke', x: -20, y: 3 },
      { act: 'run', ms: 240 },
      { act: 'poke', x: 1, y: 0 },
      { act: 'run', ms: 400 },
      { act: 'poke', x: 22, y: -5 },
      { act: 'run', ms: 1600 },
      { act: 'pointer' },
      { act: 'grab' },
      { act: 'carry', dx: 4, dy: -2 },
      { act: 'run', ms: 160 },
      { act: 'carry', dx: -9, dy: 1 },
      { act: 'run', ms: 160 },
      { act: 'toss', force: 0.8 },
      { act: 'sway', dx: 30, dy: -4, seconds: 0.016 },
      { act: 'run', ms: 16 },
      { act: 'sway', dx: 22, dy: -3, seconds: 0.016 },
      { act: 'run', ms: 16 },
      { act: 'bump', side: 'right', force: 0.7 },
      { act: 'run', ms: 800 },
      { act: 'heading', gaze: { x: -0.6, y: 0.5 } },
      { act: 'watch', gaze: { x: 0.9, y: 0.1 } },
      { act: 'run', ms: 500 },
      { act: 'heading' },
      { act: 'bump', side: 'top', force: 0.3, against: 'kit' },
      { act: 'run', ms: 900 },
      { act: 'watch' },
      { act: 'notice', kind: 'person.pet' },
      { act: 'run', ms: 700 },
      { act: 'notice', kind: 'person.pet' },
      { act: 'run', ms: 1500 },
    ],
  },
  {
    name: 'a-run-and-its-reply',
    seed: 20261111,
    acts: [
      { act: 'run', ms: 300 },
      { act: 'notice', kind: 'person.send' },
      { act: 'liveness', to: 'attend' },
      { act: 'run', ms: 500 },
      { act: 'liveness', to: 'work' },
      { act: 'feed', signals: { 'run.pace': 0, 'run.effort': 0.75, 'run.context': 0 } },
      { act: 'run', ms: 900 },
      { act: 'notice', kind: 'run.tool' },
      { act: 'feed', signals: { 'run.pace': 0.3, 'run.effort': 0.75, 'run.context': 0 } },
      { act: 'run', ms: 600 },
      { act: 'notice', kind: 'run.tool' },
      { act: 'notice', kind: 'run.tool.fail' },
      { act: 'feed', signals: { 'run.pace': 0.85, 'run.effort': 0.75, 'run.context': 0.4 } },
      { act: 'run', ms: 1400 },
      { act: 'express', expression: { tilt: 0.5, gaze: [0.3, -0.2], energy: 0.7, breath: 1.4, hue: 150 } },
      { act: 'feed', signals: { 'run.pace': 0.5, 'run.effort': 0.75, 'run.context': 1 } },
      { act: 'run', ms: 1800 },
      { act: 'express' },
      // Read aloud while it still works: the loudspeaker takes the form layer from the orb.
      { act: 'notice', kind: 'sound.start' },
      { act: 'feed', signals: sound(0.5, 1) },
      { act: 'run', ms: 400 },
      { act: 'feed', signals: sound(0.9, 0.2) },
      { act: 'notice', kind: 'sound.loud' },
      { act: 'run', ms: 500 },
      { act: 'feed', signals: quiet },
      { act: 'notice', kind: 'sound.stop' },
      { act: 'run', ms: 900 },
      { act: 'liveness', to: 'done' },
      { act: 'feed', signals: { 'run.pace': 0, 'run.effort': 0, 'run.context': 0 } },
      { act: 'run', ms: 1500 },
      { act: 'liveness', to: 'idle' },
      { act: 'run', ms: 1200 },
    ],
  },
  {
    name: 'sound-forms',
    seed: 3,
    acts: [
      ...[0, 1, 2, 3, 4].flatMap((turn): Array<Act> => [
        { act: 'notice', kind: 'sound.start' },
        { act: 'feed', signals: sound(0.3 + 0.15 * turn, turn % 2) },
        { act: 'run', ms: 700 },
        { act: 'feed', signals: sound(0.8, 0.5) },
        { act: 'run', ms: 500 },
        { act: 'feed', signals: quiet },
        { act: 'notice', kind: 'sound.stop' },
        { act: 'run', ms: 800 },
      ]),
    ],
  },
  {
    name: 'reduced-motion',
    seed: 11,
    acts: [
      { act: 'still', on: true },
      { act: 'run', ms: 200 },
      { act: 'liveness', to: 'work' },
      { act: 'feed', signals: { 'run.pace': 0.9, 'run.effort': 1, 'run.context': 0.7 } },
      { act: 'run', ms: 1200 },
      { act: 'notice', kind: 'sound.start' },
      { act: 'feed', signals: sound(0.7, 1) },
      { act: 'run', ms: 600 },
      { act: 'bump', side: 'left', force: 1 },
      { act: 'run', ms: 300 },
      { act: 'feed', signals: quiet },
      { act: 'notice', kind: 'sound.stop' },
      { act: 'liveness', to: 'idle' },
      { act: 'run', ms: 600 },
      { act: 'still', on: false },
      { act: 'run', ms: 600 },
    ],
  },
  {
    name: 'performing',
    seed: 99,
    acts: [
      { act: 'tint', hue: 30 },
      { act: 'perform', input: 'dizzy', how: { owner: 'agent' } },
      { act: 'run', ms: 500 },
      { act: 'notice', kind: 'person.poke' },
      { act: 'run', ms: 2500 },
      {
        act: 'perform',
        input: written[4]?.[1],
        how: { owner: 'agent', seed: 99 },
      },
      { act: 'feed', signals: sound(0.6, 0.8) },
      { act: 'run', ms: 1100 },
      { act: 'perform', input: written[5]?.[1], how: { owner: 'person', layer: 'reaction', intensity: 1.3 } },
      { act: 'run', ms: 700 },
      { act: 'perform', input: 'moonwalk', how: { owner: 'agent' } },
      { act: 'perform', input: { steps: [{ op: 'to', set: { 'eyes.lib': 1 } }] }, how: { owner: 'agent' } },
      { act: 'perform', input: 'sleep', how: { owner: 'agent', layer: 'mood' } },
      { act: 'feed', signals: quiet },
      { act: 'run', ms: 2600 },
      { act: 'perform', input: written[2]?.[1], how: { owner: 'agent', keep: true } },
      { act: 'run', ms: 900 },
    ],
  },
  {
    name: 'falls-asleep',
    seed: 5,
    acts: [
      { act: 'run', ms: 89_000 },
      { act: 'notice', kind: 'env.resize' },
      { act: 'run', ms: 2_000 },
      { act: 'notice', kind: 'env.theme' },
      { act: 'run', ms: 1_000 },
      { act: 'notice', kind: 'person.return' },
      { act: 'run', ms: 1_500 },
    ],
  },
]

const START = 1_700_000_000_000
const traces = scenarios.map(({ name, seed, acts }) => {
  let now = START
  const theo = new Face({ seed, clock: () => now })
  theo.attach({ gazeAt: plainGaze, go: () => {}, burst: () => {} })
  const said = (performed: { ok: boolean } & Record<string, unknown>) =>
    performed.ok
      ? { ok: true, notes: performed.notes }
      : { ok: false, path: performed.path, reason: performed.reason }
  const snapshot = () => {
    const channels: Record<string, number> = {}
    for (const channel of theo.rig.channels.names())
      channels[channel] = theo.rig.read(channel)
    const grains: Array<number> = []
    for (let index = 0; index < theo.field.count; index += 7) {
      const at = index * 8
      grains.push(
        theo.buffer[at] as number,
        theo.buffer[at + 1] as number,
        theo.buffer[at + 2] as number,
        theo.buffer[at + 7] as number,
      )
    }
    const first = [...theo.buffer.subarray(0, 8)]
    return {
      channels,
      head: theo.head(),
      emitting: theo.emitting(),
      state: theo.state(),
      shapes: theo.shapesNow().map((shape: Record<string, unknown>) => ({
        x: shape.x,
        y: shape.y,
        w: shape.w,
        h: shape.h,
        spun: shape.spun,
        stirred: shape.stirred,
      })),
      extent: theo.field.extent(),
      first,
      grains,
    }
  }
  const steps = acts.map((act) => {
    let result: unknown
    switch (act.act) {
      case 'run':
        for (let t = 0; t < act.ms; t += 8) {
          now += 8
          theo.advance(0.008)
        }
        break
      case 'notice':
        result = theo.notice(act.kind, act.weight)?.name ?? null
        break
      case 'liveness':
        theo.setLiveness(act.to)
        break
      case 'feed':
        theo.feed(act.signals)
        break
      case 'still':
        theo.still = act.on
        break
      case 'poke':
        result = theo.poke(act.x, act.y)?.name ?? null
        break
      case 'perform':
        result = said(theo.perform(act.input, act.how))
        break
      case 'pointer':
        theo.followPointer(act.gaze)
        break
      case 'watch':
        theo.watch(act.gaze)
        break
      case 'heading':
        theo.heading(act.gaze)
        break
      case 'grab':
        theo.grab()
        break
      case 'carry':
        theo.carry(act.dx, act.dy)
        break
      case 'drop':
        result = theo.drop()?.name ?? null
        break
      case 'toss':
        result = theo.toss(act.force)?.name ?? null
        break
      case 'sway':
        theo.sway(act.dx, act.dy, act.seconds)
        break
      case 'bump':
        result = theo.bump(act.side, act.force, act.against)?.name ?? null
        break
      case 'express':
        theo.express(act.expression)
        break
      case 'tint':
        theo.tint(act.hue)
        break
      case 'unform':
        theo.unform()
        break
    }
    return { act, ...(result === undefined ? {} : { result }), after: snapshot() }
  })
  return { name, seed, start: START, steps }
})
write(join(here, 'traces.json'), { source, traces })

console.log(
  `web Theo at ${commit}: ${BUILT_IN.length} performances, ${DEFAULT_RULES.length} rules, ${seats.length} shapes, ${scores.length} scores, ${traces.length} traces`,
)
