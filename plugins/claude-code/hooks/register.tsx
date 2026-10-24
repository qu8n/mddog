import type { EngineInterface, Register } from 'claude-code'

const PANE = 'mddog'
const TOOL = 'mcp__mddog__open'
const HINT = ['To scroll, hover and use the wheel or trackpad,', 'or click and press ↑↓ PgUp PgDn (Esc: back to chat)']

type Tile = { file: string; rows: number }

let file = ''
let columns = 0
let tiles: Tile[] = []
let keep = Infinity
let error = ''
let child: AsyncGenerator<unknown, unknown> | undefined
let aborts = 0

function start($: EngineInterface, cols: number) {
  void child?.return(undefined)
  tiles = []
  columns = cols
  const stream = $.process.spawn({
    argv: ['sh', '-c', 'exec "${MDDOG_BIN:-mdd}" --headless "$0" -- "$1"', String(cols), file],
  })
  child = stream
  void (async () => {
    let rest = ''
    try {
      for await (const { stream: pipe, text } of stream) {
        if (child !== stream) break
        if (pipe === 'stderr') {
          error = text.trim().split('\n').at(-1) ?? ''
          $.ui.invalidate('ui.render')
          continue
        }
        const lines = (rest + text).split('\n')
        rest = lines.pop() ?? ''
        const last = lines.at(-1)
        if (last) {
          const out = JSON.parse(last)
          tiles = out.tiles
          keep = out.keep ?? Infinity
          aborts = 0
          error = ''
          $.ui.invalidate('ui.render')
        }
      }
    } catch (e) {
      if (child === stream && aborts++ < 3) {
        child = undefined
        columns = 0
        $.ui.invalidate('ui.render')
        return
      }
      if (child === stream) error = `could not run mdd: ${String(e)}`
    }
    if (child === stream) {
      child = undefined
      error ||= 'mddog stopped. Run /mddog again to restart it.'
      $.ui.invalidate('ui.render')
    }
  })()
}

function nearest(offset: number, rows: number) {
  let top = 0
  const gaps = tiles.map((t, i) => {
    const gap = Math.max(0, top - (offset + rows), offset - (top + t.rows))
    top += t.rows
    return { i, gap }
  })
  return new Set(gaps.sort((a, b) => a.gap - b.gap).slice(0, keep).map(g => g.i))
}

async function show($: EngineInterface, path: string) {
  file = path
  tiles = []
  error = ''
  columns = 0
  aborts = 0
  const opened = await $.ui.open({ id: PANE, title: path.split('/').at(-1) ?? path })
  // An undrawn pane raises no ui.render, so stop the old file's child. A later seat (a wider terminal) renders and starts a new one.
  if (!opened.isPlaced) {
    void child?.return(undefined)
    child = undefined
  }
  $.ui.invalidate('ui.render')
  return opened
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    await $.command.register({
      name: 'mddog',
      description: 'Preview a Markdown file with mddog in a pane',
      argumentHint: '<file.md | @file.md>',
    })
    await $.tool.register({
      name: 'open',
      description:
        'Show a Markdown or MDX file in the mddog preview pane beside the chat, rendered as a browser shows it (styled like GitHub by default). Use it when the user asks to see, preview, or view a Markdown file. The pane re-renders whenever the file changes, so edit the file to update what they see.',
      inputSchema: {
        type: 'object',
        properties: { path: { type: 'string', description: 'The file, absolute or relative to the working directory' } },
        required: ['path'],
      },
      isDeferred: false,
    })
    return next(e)
  })

  on('command.run', { command: 'mddog' }, async ($, e) => {
    const arg = e.args.trim().replace(/^@/, '')
    if (!arg) return { text: 'Usage: /mddog <file.md>' }
    if (!(await $.fs.exists(arg))) return { text: `${arg} does not exist.` }
    await show($, arg)
    return {
      text: `Opened ${arg} in the mddog pane.`,
      context: [
        `The user opened ${arg} (relative to ${await $.session.cwd()}) in a mddog preview pane beside the chat. The pane re-renders whenever the file changes on disk, so to change what they see, edit that file with your usual tools.`,
      ],
    }
  })

  on('tool.call', { tool: TOOL }, async ($, e) => {
    const path = typeof e.path === 'string' ? e.path.trim() : ''
    if (!path) return { deny: 'path is required: the Markdown file to show' }
    if (!(await $.fs.exists(path))) return { deny: `${path} does not exist` }
    const opened = await show($, path)
    if (!opened.isPlaced) {
      const message = `Claude Code requires a wider terminal view. Zoom out until the pane appears or run \`/mddog ${path}\`.`
      return {
        result: `mddog error: ${opened.reason}. Send user this message verbatim, with nothing added:\n\n${message}`,
      }
    }
    return { result: `Showing ${path} in the mddog pane beside the chat. It re-renders whenever the file changes on disk.` }
  })

  on('session.end', ($, e, next) => {
    void child?.return(undefined)
    child = undefined
    file = ''
    tiles = []
    return next(e)
  })

  on('ui.close', { id: PANE }, ($, e, next) => {
    void child?.return(undefined)
    child = undefined
    return next(e)
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e, next) => {
    if (e.surface !== 'terminal') return next(e)
    const { Box, Text, Image } = $.ui.resolve(e)
    const cols = Math.min(255, e.props.bodyColumns)
    if (file && cols > 0 && cols !== columns) start($, cols)
    if (!file) return <Text dimColor>Run /mddog {'<file.md>'} to preview a file here.</Text>
    if (tiles.length === 0) return error ? <Text color="red">{error}</Text> : <Text dimColor>Rendering {file}…</Text>
    const tall = tiles.reduce((sum, t) => sum + t.rows, 0) > e.props.scroll.bodyRows
    const near = nearest(e.props.scroll.offset, e.props.scroll.bodyRows)
    return (
      <Box flexDirection="column">
        {error && <Text color="red">{error}</Text>}
        {tall && HINT.map((line, i) => <Text key={`hint-${i}`} dimColor wrap="truncate-end">{line}</Text>)}
        {tiles.map((t, i) => !near.has(i) ? <Box key={`gap-${i}`} height={t.rows} /> : (
          <Image key={`tile-${i}`} source={{ file: t.file, format: 'png' }} columns={columns} rows={t.rows} alt={i === 0 ? `${file} (this terminal cannot show images)` : ' '} />
        ))}
      </Box>
    )
  })
}
