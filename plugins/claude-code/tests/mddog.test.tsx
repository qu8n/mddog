import { expect, mock, test } from 'claude-code/testing'

const PANE = {
  plugin: 'mddog',
  component: 'Pane',
  requestId: 'mddog',
  surface: 'terminal',
  props: {
    title: 'doc.md',
    isFocused: false,
    bodyColumns: 40,
    placement: 'dock',
    scroll: { offset: 0, bodyRows: 20 },
    view: {},
  },
} as const

const mddog = (args: string) =>
  ({ command: 'mddog', args, origin: { kind: 'composer' }, presentation: { isFullscreen: true, columns: 160 } }) as const

test('/mddog shows the tiles mddog prints and tells the model the file', async ($, on) => {
  const spawned: string[][] = []
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.cwd', () => ({ value: '/work' }))
  on('fs.exists', (_, e) => ({ value: e.path.endsWith('doc.md') }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('process.spawn', async function* (_, e) {
    spawned.push([...e.argv])
    yield { stream: 'stdout', text: '{"columns":40,"tiles":[{"file":"/tmp/t/1-0.png","rows":100},' }
    yield { stream: 'stdout', text: '{"file":"/tmp/t/1-1.png","rows":7}]}\n' }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })
  expect((await $.command.run(mddog('typo.md'))).text).toContain('does not exist')
  const ran = await $.command.run(mddog('doc.md'))
  expect(ran.context?.join(' ')).toContain('doc.md')
  expect((await $.command.run(mddog('@doc.md'))).text).toBe('Opened doc.md in the mddog pane.')

  const ui = await $.ui.mount(PANE)
  expect(spawned[0]?.slice(-2)).toEqual(['40', 'doc.md'])
  const tile = await ui.find({ type: 'Image', key: 'tile-1' })
  expect(tile?.props.rows).toBe(7)
  expect(await ui.find({ type: 'Text', text: /To scroll|PgUp PgDn/ })).toBeFalsy()
  expect((await ui.find({ type: 'Text', text: /stopped/ }))?.text).toContain('Run /mddog again')
  await ui.unmount()
})

test('the model opens a file in the pane with the open tool', async ($, on) => {
  const spawned: string[][] = []
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('tool.register', (_, e) => ({ value: { tool: `mcp__mddog__${e.name}` } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('fs.exists', (_, e) => ({ value: e.path.endsWith('notes/plan.md') }))
  on('process.spawn', async function* (_, e) {
    spawned.push([...e.argv])
    yield { stream: 'stdout', text: '{"columns":40,"tiles":[{"file":"/tmp/t/1-0.png","rows":12}]}\n' }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })
  const missing = await $.tool.call({ tool: 'mcp__mddog__open', path: ' ' })
  expect(missing.deny).toContain('path')
  expect((await $.tool.call({ tool: 'mcp__mddog__open', path: 'nope.md' })).deny).toContain('does not exist')

  const ran = await $.tool.call({ tool: 'mcp__mddog__open', path: 'notes/plan.md' })
  expect(String(ran.result)).toContain('notes/plan.md')

  const ui = await $.ui.mount({ ...PANE, props: { ...PANE.props, title: 'plan.md' } })
  expect(spawned[0]?.slice(-2)).toEqual(['40', 'notes/plan.md'])
  expect((await ui.find({ type: 'Image', key: 'tile-0' }))?.props.rows).toBe(12)
  await ui.unmount()
})

test('only the tiles nearest the view are mounted, so the images stay under the terminal storage limit', async ($, on) => {
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.cwd', () => ({ value: '/work' }))
  on('fs.exists', () => ({ value: true }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  const tiles = Array.from({ length: 10 }, (_, i) => ({ file: `/tmp/t/1-${i}.png`, rows: 50 }))
  on('process.spawn', async function* () {
    yield { stream: 'stdout', text: `${JSON.stringify({ columns: 40, keep: 3, tiles })}\n` }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })
  await $.command.run(mddog('doc.md'))
  const top = await $.ui.mount(PANE)
  for (const i of [0, 1, 2]) expect(await top.find({ type: 'Image', key: `tile-${i}` })).toBeTruthy()
  expect(await top.find({ type: 'Image', key: 'tile-3' })).toBeFalsy()
  await top.unmount()

  const middle = await $.ui.mount({ ...PANE, props: { ...PANE.props, scroll: { offset: 260, bodyRows: 20 } } })
  for (const i of [4, 5, 6]) expect(await middle.find({ type: 'Image', key: `tile-${i}` })).toBeTruthy()
  for (const i of [0, 3, 7]) expect(await middle.find({ type: 'Image', key: `tile-${i}` })).toBeFalsy()
  await middle.unmount()
})

test('a child that a newer render aborts starts again', async ($, on) => {
  let spawns = 0
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.cwd', () => ({ value: '/work' }))
  on('fs.exists', () => ({ value: true }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('process.spawn', async function* () {
    if (++spawns === 1) throw new Error('aborted')
    yield { stream: 'stdout', text: '{"columns":40,"tiles":[{"file":"/tmp/t/1-0.png","rows":12}]}\n' }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })
  await $.command.run(mddog('doc.md'))
  const ui = await $.ui.mount(PANE)
  expect((await ui.find({ type: 'Image', key: 'tile-0' }))?.props.rows).toBe(12)
  expect(spawns).toBe(2)
  expect(await ui.find({ type: 'Text', text: /could not run/ })).toBeFalsy()
  await ui.unmount()
})

test('a child that keeps failing to start ends in an error until the next /mddog', async ($, on) => {
  let spawns = 0
  const clock = mock.clock(on)
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.cwd', () => ({ value: '/work' }))
  on('fs.exists', () => ({ value: true }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  let failing = true
  on('process.spawn', async function* () {
    spawns++
    if (failing || spawns === 5) throw new Error('aborted')
    yield { stream: 'stdout', text: '{"columns":40,"tiles":[{"file":"/tmp/t/1-0.png","rows":12}]}\n' }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })
  await $.command.run(mddog('doc.md'))
  const ui = await $.ui.mount(PANE)
  for (let i = 0; i < 4; i++) await clock.settle()
  expect((await ui.find({ type: 'Text', text: /could not run/ }))?.text).toContain('could not run mdd:')
  expect(spawns).toBe(4)

  failing = false
  await $.command.run(mddog('doc.md'))
  for (let i = 0; i < 4; i++) await clock.settle()
  expect((await ui.find({ type: 'Image', key: 'tile-0' }))?.props.rows).toBe(12)
  expect(spawns).toBe(6)
  await ui.unmount()
})

test('the open tool says so when the pane waits undrawn on a narrow terminal', async ($, on) => {
  let spawns = 0
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('tool.register', (_, e) => ({ value: { tool: `mcp__mddog__${e.name}` } }))
  on('ui.open', () => ({ value: { isPlaced: false, reason: 'unasked panes need 144 columns, the terminal has 100' } }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('fs.exists', () => ({ value: true }))
  on('process.spawn', async function* () {
    spawns++
    yield { stream: 'stdout', text: '{"columns":40,"tiles":[{"file":"/tmp/t/1-0.png","rows":12}]}\n' }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })
  const ran = String((await $.tool.call({ tool: 'mcp__mddog__open', path: 'doc.md' })).result)
  expect(ran).toContain('mddog error')
  expect(ran).toContain('144 columns')
  expect(ran).toContain('Zoom out until the pane appears')
  expect(ran).toContain('/mddog doc.md')
  expect(ran).not.toContain('Showing')
  expect(spawns).toBe(0)

  // A wider terminal seats the waiting pane later, and its first render starts mdd.
  const ui = await $.ui.mount(PANE)
  expect((await ui.find({ type: 'Image', key: 'tile-0' }))?.props.rows).toBe(12)
  expect(spawns).toBe(1)
  await ui.unmount()
})

test('the desktop pane draws WebP tiles as Svgs of a set CSS size, a window of them under the drawing limit', async ($, on) => {
  const spawned: string[][] = []
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.cwd', () => ({ value: '/work' }))
  on('fs.exists', () => ({ value: true }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  // 15 tiles of 87 columns at 2x, 1392x640 px, each 45,000 base64 characters: about what mdd --webp writes for fixtures/gfm.md.
  on('fs.read', () => ({ value: { base64: 'A'.repeat(45_000) } }))
  const tiles = Array.from({ length: 15 }, (_, i) => ({ file: `/tmp/t/1-${i}.webp`, rows: 20, width: 1392, height: 640, bytes: 33_750 }))
  on('process.spawn', async function* (_, e) {
    spawned.push([...e.argv])
    yield { stream: 'stdout', text: `${JSON.stringify({ columns: 87, tiles })}\n` }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: true })
  await $.command.run(mddog('doc.md'))
  const props = { ...PANE.props, bodyColumns: 80, scroll: { offset: 0, bodyRows: 40 } }
  const top = await $.ui.mount({ ...PANE, surface: 'desktop', props })
  expect(spawned[0]?.join(' ')).toContain('--webp')
  expect(spawned[0]?.slice(-2)).toEqual(['87', 'doc.md'])
  const all = (await top.findAll({ type: 'Svg' })).map(s => s.props as { source: string; alt: string; width?: number; height?: number; isInteractive?: boolean })
  const drawn = all.filter(s => s.source.includes('<image'))
  expect(drawn.length).toBe(5)
  expect(drawn[0]?.source).toContain('href="data:image/webp;base64,AAAA')
  expect(all.reduce((n, s) => n + s.source.length, 0)).toBeLessThan(262_144)
  // 80 columns at 8.75 CSS px lays out 87 mdd columns, drawn 696 CSS px wide: every Svg the same, tiles at 1x.
  expect(all.every(s => s.width === 696 && !s.isInteractive && s.source.includes('width="1392"'))).toBe(true)
  expect(drawn.every(s => s.height === 320)).toBe(true)
  // The gaps add up to the tiles they stand for, in Svgs no taller than 4096 px.
  const heights = all.map(s => Number(/height="(\d+)"/.exec(s.source)?.[1]))
  expect(heights.reduce((a, b) => a + b, 0)).toBe(15 * 640)
  expect(Math.max(...heights)).toBeLessThanOrEqual(4096)
  expect(all.reduce((n, s) => n + (s.height ?? 0), 0)).toBe(15 * 320)
  // The desktop draws an Svg with no content and a blank alt as nothing, so each gap has both.
  expect(all.filter(s => !s.source.includes('<image')).every(s => s.source.includes('<rect') && s.alt.trim() !== '')).toBe(true)
  await top.unmount()

  // About 16.4 rows a tile (320 CSS px over 19.5 px a row), so offset 160 is in tile 10.
  const deep = await $.ui.mount({ ...PANE, surface: 'desktop', props: { ...props, scroll: { offset: 160, bodyRows: 40 } } })
  const alts = (await deep.findAll({ type: 'Svg' })).map(s => (s.props as { alt: string }).alt)
  expect(alts).toContain('doc.md, part 10 of 15')
  expect(alts).not.toContain('doc.md, part 1 of 15')
  await deep.unmount()
})

test('the desktop pane keeps a long page under the drawing limit, gaps included', async ($, on) => {
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.cwd', () => ({ value: '/work' }))
  on('fs.exists', () => ({ value: true }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  let reads = 0
  on('fs.read', () => {
    reads++
    return { value: { base64: 'A'.repeat(45_000) } }
  })
  // 3,000 tiles: about 470 gap Svgs of 4096 px, past the room the tiles leave.
  const tiles = Array.from({ length: 3000 }, (_, i) => ({ file: `/tmp/t/1-${i}.webp`, rows: 20, width: 1600, height: 640, bytes: 33_750 }))
  on('process.spawn', async function* () {
    yield { stream: 'stdout', text: `${JSON.stringify({ columns: 90, tiles })}\n` }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: true })
  await $.command.run(mddog('doc.md'))
  const ui = await $.ui.mount({ ...PANE, surface: 'desktop', props: { ...PANE.props, bodyColumns: 80, scroll: { offset: 15_000, bodyRows: 40 } } })
  const all = (await ui.findAll({ type: 'Svg' })).map(s => String((s.props as { source: string }).source))
  expect(all.some(s => s.includes('<image'))).toBe(true)
  expect(all.reduce((n, s) => n + s.length, 0)).toBeLessThan(262_144)
  // Only the tiles drawn are read, not all 3,000.
  expect(reads).toBeLessThan(10)
  await ui.unmount()
})

test('the desktop pane draws the tiles in view first, and leaves a tile too big for one Svg a gap', async ($, on) => {
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.cwd', () => ({ value: '/work' }))
  on('fs.exists', () => ({ value: true }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('fs.read', (_, e) => ({ value: { base64: 'A'.repeat(e.path.endsWith('-3.webp') ? 133_336 : 90_000) } }))
  // Each tile about 14.3 rows (640 px of 1600 at 87 columns), so a view of 40 rows from the top shows tiles 0 to 2. Tile 3 is past 131,072 characters.
  const tiles = Array.from({ length: 6 }, (_, i) => ({ file: `/tmp/t/1-${i}.webp`, rows: 20, width: 1600, height: 640, bytes: i === 3 ? 100_002 : 67_500 }))
  on('process.spawn', async function* () {
    yield { stream: 'stdout', text: `${JSON.stringify({ columns: 90, tiles })}\n` }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: true })
  await $.command.run(mddog('doc.md'))
  const props = { ...PANE.props, bodyColumns: 80, scroll: { offset: 0, bodyRows: 40 } }
  const top = await $.ui.mount({ ...PANE, surface: 'desktop', props })
  const alts = (await top.findAll({ type: 'Svg' })).map(s => (s.props as { alt: string }).alt)
  // Two 90,000-character tiles fill the budget: the first two in view, not one each side.
  expect(alts.filter(a => /part \d+ of/.test(a))).toEqual(['doc.md, part 1 of 6', 'doc.md, part 2 of 6'])
  await top.unmount()

  const at3 = await $.ui.mount({ ...PANE, surface: 'desktop', props: { ...props, scroll: { offset: 45, bodyRows: 10 } } })
  const svgs = (await at3.findAll({ type: 'Svg' })).map(s => s.props as { alt: string; source: string })
  expect(svgs.map(s => s.alt)).not.toContain('doc.md, part 4 of 6')
  expect(svgs.every(s => s.source.length <= 131_072)).toBe(true)
  await at3.unmount()
})

test('the terminal and the desktop draw the pane at once, each from its own mdd', async ($, on) => {
  const spawned: string[][] = []
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.cwd', () => ({ value: '/work' }))
  on('fs.exists', () => ({ value: true }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('fs.read', () => ({ value: { base64: 'AAAA' } }))
  on('process.spawn', async function* (_, e) {
    spawned.push([...e.argv])
    const webp = e.argv.join(' ').includes('--webp')
    const tile = webp ? { file: '/tmp/t/1-0.webp', rows: 20, width: 1392, height: 640, bytes: 3 } : { file: '/tmp/t/1-0.png', rows: 20 }
    yield { stream: 'stdout', text: `${JSON.stringify({ columns: 40, tiles: [tile] })}\n` }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })
  await $.command.run(mddog('doc.md'))
  for (let round = 0; round < 3; round++) {
    const term = await $.ui.mount(PANE)
    const desk = await $.ui.mount({ ...PANE, surface: 'desktop', props: { ...PANE.props, bodyColumns: 80 } })
    await term.unmount()
    await desk.unmount()
  }
  expect(spawned.length).toBe(2)
  expect(spawned.map(a => a.join(' ').includes('--webp')).sort()).toEqual([false, true])
})

test('the desktop pane covers the view and a tile each side with small copies, then sharpens the view while it fits', async ($, on) => {
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.cwd', () => ({ value: '/work' }))
  on('fs.exists', () => ({ value: true }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('fs.read', (_, e) => ({ value: { base64: 'A'.repeat(e.path.endsWith('s.webp') ? 13_336 : 80_000) } }))
  // 10 tiles of 160 CSS px (about 8.2 rows): a 40-row view shows tiles 0 to 4. Sharp ones take 80,000
  // characters, so only two fit; small copies take 13,336.
  const tiles = Array.from({ length: 10 }, (_, i) => ({
    file: `/tmp/t/1-${i}.webp`, rows: 10, width: 1392, height: 320, bytes: 60_000,
    small: { file: `/tmp/t/1-${i}s.webp`, width: 696, height: 160, bytes: 10_002 },
  }))
  on('process.spawn', async function* () {
    yield { stream: 'stdout', text: `${JSON.stringify({ columns: 87, tiles })}\n` }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: true })
  await $.command.run(mddog('doc.md'))
  const ui = await $.ui.mount({ ...PANE, surface: 'desktop', props: { ...PANE.props, bodyColumns: 80, scroll: { offset: 0, bodyRows: 40 } } })
  const svgs = (await ui.findAll({ type: 'Svg' })).map(s => s.props as { alt: string; source: string; height: number })
  const part = (n: number) => svgs.find(s => s.alt === `doc.md, part ${n} of 10`)
  // The view and the tile below it are all drawn: none is left a gap.
  for (const n of [1, 2, 3, 4, 5, 6]) expect(part(n)).toBeDefined()
  expect(part(1)?.source).toContain('width="1392"')
  expect(part(2)?.source).toContain('width="1392"')
  expect(part(3)?.source).toContain('width="696"')
  // A small copy takes the same CSS size as the sharp tile.
  expect(part(3)?.height).toBe(part(1)?.height)
  expect(svgs.reduce((n, s) => n + s.source.length, 0)).toBeLessThan(262_144)
  await ui.unmount()
})

test('a scroll on the desktop measures the row, so a deep offset draws the tiles in view', async ($, on) => {
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.cwd', () => ({ value: '/work' }))
  on('fs.exists', () => ({ value: true }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('fs.read', () => ({ value: { base64: 'A'.repeat(100_000) } }))
  on('ui.scroll', () => ({}))
  // 10 tiles of 160 CSS px, 1,600 in all, at 100,000 characters: two fit a drawing.
  const tiles = Array.from({ length: 10 }, (_, i) => ({ file: `/tmp/t/1-${i}.webp`, rows: 10, width: 1392, height: 320, bytes: 75_000 }))
  on('process.spawn', async function* () {
    yield { stream: 'stdout', text: `${JSON.stringify({ columns: 87, tiles })}\n` }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: true })
  await $.command.run(mddog('doc.md'))
  const props = { ...PANE.props, bodyColumns: 80, scroll: { offset: 0, bodyRows: 4 } }
  await (await $.ui.mount({ ...PANE, surface: 'desktop', props })).unmount()
  // 1,600 CSS px over 64 rows: 25 px a row, not the 19.5 assumed.
  await $.ui.scroll({ component: 'Pane', requestId: 'mddog', offset: 1, by: 1, bodyRows: 4, contentRows: 64, origin: { kind: 'person' } })
  // Row 40 is at 1,000 CSS px, in tile 7; at 19.5 px it would be tile 5.
  const deep = await $.ui.mount({ ...PANE, surface: 'desktop', props: { ...props, scroll: { offset: 40, bodyRows: 4 } } })
  const alts = (await deep.findAll({ type: 'Svg' })).map(s => (s.props as { alt: string }).alt)
  expect(alts).toContain('doc.md, part 7 of 10')
  expect(alts).not.toContain('doc.md, part 5 of 10')
  await deep.unmount()
})

test('once scrolling stops, the margins give way so the view goes sharp', async ($, on) => {
  const clock = mock.clock(on)
  on('command.register', () => ({ value: { command: 'mddog' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.cwd', () => ({ value: '/work' }))
  on('fs.exists', () => ({ value: true }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('ui.scroll', () => ({}))
  on('fs.read', (_, e) => ({ value: { base64: 'A'.repeat(e.path.endsWith('s.webp') ? 26_668 : 114_668) } }))
  // Tiles of 160 CSS px (8.2 rows): a 16-row view shows tiles 0 and 1. Two sharp ones and two small ones
  // fit the budget, but not two sharp ones and three small ones.
  const tiles = Array.from({ length: 6 }, (_, i) => ({
    file: `/tmp/t/1-${i}.webp`, rows: 10, width: 1392, height: 320, bytes: 86_000,
    small: { file: `/tmp/t/1-${i}s.webp`, width: 696, height: 160, bytes: 20_000 },
  }))
  on('process.spawn', async function* () {
    yield { stream: 'stdout', text: `${JSON.stringify({ columns: 87, tiles })}\n` }
    return { value: { code: 0, signal: null } }
  })

  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: true })
  await $.command.run(mddog('doc.md'))
  const props = { ...PANE.props, bodyColumns: 80, scroll: { offset: 0, bodyRows: 16 } }
  const width = async () => {
    const ui = await $.ui.mount({ ...PANE, surface: 'desktop', props })
    const svgs = (await ui.findAll({ type: 'Svg' })).map(s => s.props as { alt: string; source: string })
    await ui.unmount()
    return (n: number) => /width="(\d+)"/.exec(svgs.find(s => s.alt === `doc.md, part ${n} of 6`)?.source ?? '')?.[1]
  }
  await width()
  await $.ui.scroll({ component: 'Pane', requestId: 'mddog', offset: 0, by: 1, bodyRows: 16, contentRows: 50, origin: { kind: 'person' } })
  const moving = await width()
  expect([moving(1), moving(2), moving(3)]).toEqual(['1392', '696', '696'])
  await clock.advance(300)
  const settled = await width()
  expect([settled(1), settled(2)]).toEqual(['1392', '1392'])
})
