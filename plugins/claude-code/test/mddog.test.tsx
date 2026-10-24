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
  expect((await $.command.run({ command: 'mddog', args: 'typo.md' })).text).toContain('does not exist')
  const ran = await $.command.run({ command: 'mddog', args: 'doc.md' })
  expect(ran.context?.join(' ')).toContain('doc.md')
  expect((await $.command.run({ command: 'mddog', args: '@doc.md' })).text).toBe('Opened doc.md in the mddog pane.')

  const ui = await $.ui.mount(PANE)
  expect(spawned[0]?.slice(-2)).toEqual(['40', 'doc.md'])
  const tile = await ui.find({ type: 'Image', key: 'tile-1' })
  expect(tile?.props.rows).toBe(7)
  expect((await ui.find({ type: 'Text', text: /^To scroll/ }))?.text).toBe('To scroll, hover and use the wheel or trackpad,')
  expect((await ui.find({ type: 'Text', text: /^or click/ }))?.text).toBe('or click and press ↑↓ PgUp PgDn (Esc: back to chat)')
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
  expect(await ui.find({ type: 'Text', text: /To scroll|PgUp PgDn/ })).toBeFalsy()
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
  await $.command.run({ command: 'mddog', args: 'doc.md' })
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
  await $.command.run({ command: 'mddog', args: 'doc.md' })
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
  await $.command.run({ command: 'mddog', args: 'doc.md' })
  const ui = await $.ui.mount(PANE)
  for (let i = 0; i < 4; i++) await clock.settle()
  expect((await ui.find({ type: 'Text', text: /could not run/ }))?.text).toContain('could not run mdd:')
  expect(spawns).toBe(4)

  failing = false
  await $.command.run({ command: 'mddog', args: 'doc.md' })
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
