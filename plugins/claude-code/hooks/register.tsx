import type { EngineInterface, Register, Timer } from 'claude-code'

const PANE = 'mddog'
const TOOL = 'mcp__mddog__open'

// A tile's picture: the sharp one at the top level, and on the desktop a small copy at 1x.
type Picture = { file: string; width?: number; height?: number; bytes?: number }
type Tile = Picture & { rows: number; small?: Picture }

// The desktop app has no Image element. It draws an Svg as <img src="data:image/svg+xml,...">
// without the scrub it gives interactive ones, so an <image> with a data: WebP inside shows.
// One Svg's source is at most 131,072 characters, and the desktop drops a whole drawing at
// 262,144 characters, so only a window of tiles near the view is read and drawn.
const SVG_MAX_CHARS = 131_072
// Under 262,144 with room to spare: each Svg is charged about 550 characters of markup and props for some 310.
const SVG_BUDGET = 250_000
// The desktop refuses an Svg taller than this, so a long gap is cut into several.
const SVG_MAX_PX = 4096
// A gap Svg's source (about 150 characters) and its props (key, alt, width, height) as the drawing
// serializes them.
const GAP_CHARS = 300
// CSS px per desktop column and row: the code font's advance and line height. Live, 61 and 202 columns
// put the advance at 8.8 to 9; this stays under it, since a tile wider than the pane is cut. ROW_PX only
// picks the tiles near the view until the first scroll measures the real one (View.rowPx).
const COLUMN_PX = 8.75
const ROW_PX = 19.5
// How long after the last scroll the margins give way to sharp tiles in view.
const SETTLE_MS = 300
// CSS px per mdd column: tiles are drawn at this, so the 2x pixels map one to one.
const MDD_COLUMN_PX = 8

// One mdd child and its tiles per surface: a session can draw the pane on the terminal and on the
// mobile app at once, at different widths and in different formats.
type View = {
  columns: number
  tiles: Tile[]
  keep: number
  error: string
  child?: AsyncGenerator<unknown, unknown>
  aborts: number
  // The Svg sources of the tiles last drawn, by file, so a scroll reads only the tiles new to the view.
  sources: Map<string, string>
  // CSS px per scroll row, as a ui.scroll measured it.
  rowPx?: number
  // Set while the person scrolls, cleared a moment after: margins are drawn only while it is set.
  moving?: Timer
}

let file = ''
const views = new Map<string, View>()

function view(surface: string) {
  let v = views.get(surface)
  if (!v) views.set(surface, (v = { columns: 0, tiles: [], keep: Infinity, error: '', aborts: 0, sources: new Map() }))
  return v
}

function stop() {
  for (const v of views.values()) {
    void v.child?.return(undefined)
    v.child = undefined
    // A render at the same width starts a new child.
    v.columns = 0
    v.moving?.cancel()
    v.moving = undefined
  }
}

// Every Svg gets its CSS width and height. Without them the desktop sizes the box from its own guess
// at the pane's width but draws the image at the real one, so tiles overlap or leave gaps.
function svg(w: number, h: number, body = '') {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${body}</svg>`
}

// The Svg source's length: base64 takes 4 characters per 3 bytes, and the markup under 250.
function chars(t: Picture) {
  return 4 * Math.ceil((t.bytes ?? 0) / 3) + 250
}

async function source($: EngineInterface, t: Picture) {
  const read = await $.fs.read(t.file, { as: 'bytes' })
  return svg(t.width ?? 0, t.height ?? 0, `<image width="${t.width}" height="${t.height}" href="data:image/webp;base64,${read.base64}"/>`)
}

function start($: EngineInterface, v: View, cols: number, webp: boolean) {
  void v.child?.return(undefined)
  v.tiles = []
  v.sources = new Map()
  v.columns = cols
  const stream = $.process.spawn({
    argv: ['sh', '-c', `exec "\${MDDOG_BIN:-mdd}" --headless "$0"${webp ? ' --webp' : ''} -- "$1"`, String(cols), file],
  })
  v.child = stream
  void (async () => {
    let rest = ''
    try {
      for await (const { stream: pipe, text } of stream) {
        if (v.child !== stream) break
        if (pipe === 'stderr') {
          v.error = text.trim().split('\n').at(-1) ?? ''
          $.ui.invalidate('ui.render')
          continue
        }
        const lines = (rest + text).split('\n')
        rest = lines.pop() ?? ''
        const last = lines.at(-1)
        if (last) {
          const out = JSON.parse(last)
          v.tiles = out.tiles
          v.keep = out.keep ?? Infinity
          v.aborts = 0
          v.error = ''
          $.ui.invalidate('ui.render')
        }
      }
    } catch (e) {
      if (v.child === stream && v.aborts++ < 3) {
        v.child = undefined
        v.columns = 0
        $.ui.invalidate('ui.render')
        return
      }
      if (v.child === stream) v.error = `couldn't run mdd: ${String(e)}`
    }
    if (v.child === stream) {
      v.child = undefined
      v.error ||= 'mddog stopped. Run /mddog again to restart it.'
      $.ui.invalidate('ui.render')
    }
  })()
}

function nearest({ tiles, keep }: View, offset: number, rows: number) {
  let top = 0
  const gaps = tiles.map((t, i) => {
    const gap = Math.max(0, top - (offset + rows), offset - (top + t.rows))
    top += t.rows
    return { i, gap }
  })
  return new Set(gaps.sort((a, b) => a.gap - b.gap).slice(0, keep).map(g => g.i))
}

// The picture to draw for each tile near the view. Small copies cover the view first, and a tile each side
// while the person scrolls; then those go sharp, and tiles further out are added small, while the budget
// lasts. Once scrolling stops the margins give way, so a view full of pictures can go all sharp. A picture
// too big for one Svg is never drawn.
function window({ tiles, columns, rowPx = ROW_PX, moving }: View, offset: number, bodyRows: number) {
  const rows = (t: Tile) => css(columns, t, t.height ?? 0) / rowPx
  let top = 0
  let at = tiles.length
  let end = tiles.length - 1
  for (const [i, t] of tiles.entries()) {
    if (at === tiles.length && offset < top + rows(t)) at = i
    top += rows(t)
    if (top >= offset + bodyRows) {
      end = i
      break
    }
  }
  at = Math.min(at, end)
  const order = []
  for (let i = at; i <= end; i++) order.push(i)
  for (let step = 1; at - step >= 0 || end + step < tiles.length; step++) order.push(end + step, at - step)
  const near = new Map<number, Picture>()
  // A gap Svg costs under GAP_CHARS. There is a run of gaps before each drawn tile and one after the last,
  // and a run takes one more Svg per 4096 px.
  const height = tiles.reduce((sum, t) => sum + (t.height ?? 0), 0)
  let spent = GAP_CHARS * (1 + Math.ceil(height / SVG_MAX_PX))
  const put = (i: number, p: Picture | undefined) => {
    const was = near.get(i)
    const cost = p ? chars(p) - (was ? chars(was) : -GAP_CHARS) : 0
    if (!p || chars(p) > SVG_MAX_CHARS || spent + cost > SVG_BUDGET) return
    spent += cost
    near.set(i, p)
  }
  const needed = order.slice(0, end - at + (moving ? 3 : 1))
  for (const i of needed) put(i, tiles[i]?.small ?? tiles[i])
  for (const i of needed) if (tiles[i]?.small) put(i, tiles[i])
  for (const i of order.slice(needed.length)) put(i, tiles[i]?.small ?? tiles[i])
  return near
}

// Blank Svgs as tall as the run of tiles they stand for. The desktop drew an Svg with no content and a
// blank alt as nothing at all, so the page below moved up and the scroll ended early: each gap holds a
// clear rect and says what it stands for.
function gap(columns: number, run: Tile[]) {
  const t = run[0]
  const parts: { source: string; height: number }[] = []
  if (!t) return parts
  for (let h = run.reduce((sum, t) => sum + (t.height ?? 0), 0); h > 0; h -= SVG_MAX_PX) {
    const part = Math.min(h, SVG_MAX_PX)
    parts.push({ source: svg(t.width ?? 0, part, '<rect width="100%" height="100%" fill-opacity="0"/>'), height: css(columns, t, part) })
  }
  return parts
}

// A tile's pixels in CSS px: the tile is `columns` mdd columns wide.
function css(columns: number, t: Tile, px: number) {
  return (px * columns * MDD_COLUMN_PX) / (t.width || 1)
}

async function show($: EngineInterface, path: string) {
  file = path
  for (const v of views.values()) Object.assign(v, { tiles: [], error: '', columns: 0, aborts: 0 })
  const opened = await $.ui.open({ id: PANE, title: path.split('/').at(-1) ?? path })
  // An undrawn pane raises no ui.render, so stop the old file's child. A later seat (a wider terminal) renders and starts a new one.
  if (!opened.isPlaced) stop()
  $.ui.invalidate('ui.render')
  return opened
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    await Promise.all([
      $.command.register({
        name: 'mddog',
        description: 'Preview a Markdown file with mddog in a pane',
        argumentHint: '<file.md | @file.md>',
      }),
      $.tool.register({
        name: 'open',
        description:
          'Show a Markdown file in the mddog preview pane beside the chat, rendered as a browser shows it (styled like GitHub by default). Use it when the user asks to see, preview, or view a Markdown file. The pane re-renders whenever the file changes, so edit the file to update what they see.',
        inputSchema: {
          type: 'object',
          properties: { path: { type: 'string', description: 'The file, absolute or relative to the working directory' } },
          required: ['path'],
        },
        isDeferred: false,
      }),
    ])
    return next(e)
  })

  on('command.run', { command: 'mddog' }, async ($, e) => {
    const arg = e.args.trim().replace(/^@/, '')
    if (!arg) return { text: 'Usage: /mddog <file.md>' }
    if (!(await $.fs.exists(arg))) return { text: `${arg} does not exist.` }
    const [, cwd] = await Promise.all([show($, arg), $.session.cwd()])
    return {
      text: `Opened ${arg} in the mddog pane.`,
      context: [
        `The user opened ${arg} (relative to ${cwd}) in a mddog preview pane beside the chat. The pane re-renders whenever the file changes on disk, so to change what they see, edit that file with your usual tools.`,
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
    stop()
    views.clear()
    file = ''
    return next(e)
  })

  on('ui.close', { id: PANE }, ($, e, next) => {
    stop()
    return next(e)
  })

  // A scroll says how many rows the tree takes, and the tree is all Svgs of known CSS height, so it measures
  // the row: offsets then map to the right tiles however far down. The event names no surface, so it
  // measures only while one surface draws tiles.
  on('ui.scroll', { component: 'Pane', requestId: PANE }, ($, e, next) => {
    const drawing = [...views.values()].filter(v => v.tiles.length > 0)
    const v = drawing.length === 1 ? drawing[0] : undefined
    if (v?.tiles[0]?.width && e.contentRows > 0) {
      const px = v.tiles.reduce((sum, t) => sum + css(v.columns, t, t.height ?? 0), 0) / e.contentRows
      if (px > 10 && px < 40) v.rowPx = px
    }
    for (const v of drawing) {
      v.moving?.cancel()
      v.moving = $.clock.after(SETTLE_MS, () => {
        v.moving = undefined
        $.ui.invalidate('ui.render')
      })
    }
    return next(e)
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e, next) => {
    if (e.surface === 'desktop' || e.surface === 'vscode' || e.surface === 'mobile') {
      const { Box, Text, Svg } = $.ui.resolve(e)
      const v = view(e.surface)
      const cols = Math.min(255, Math.floor((e.props.bodyColumns * COLUMN_PX) / MDD_COLUMN_PX))
      if (file && cols > 0 && cols !== v.columns) start($, v, cols, true)
      if (!file) return <Text dimColor>Run /mddog {'<file.md>'} to preview a file here.</Text>
      if (v.tiles.length === 0) return v.error ? <Text color="red">{v.error}</Text> : <Text dimColor>Rendering {file}…</Text>
      // The read below waits, and a new round may land meanwhile, so the drawing uses this round alone.
      const { tiles, columns } = v
      const near = window(v, e.props.scroll.offset, e.props.scroll.bodyRows)
      const read = new Map<string, string>()
      await Promise.all([...near.values()].map(async p => {
        // A file the next round already swept is left a gap until that round's tiles arrive.
        const s = v.sources.get(p.file) ?? (await source($, p).catch(() => undefined))
        if (s) read.set(p.file, s)
      }))
      if (v.tiles === tiles) v.sources = read
      const drawn: { key: string; source: string; alt: string; height: number }[] = []
      let run: Tile[] = []
      const flush = (at: number) => {
        drawn.push(...gap(columns, run).map((g, n) => ({ key: `gap-${at}-${n}`, alt: 'Not shown yet', ...g })))
        run = []
      }
      for (const [i, t] of tiles.entries()) {
        const s = read.get(near.get(i)?.file ?? '')
        if (!s) {
          run.push(t)
          continue
        }
        flush(i)
        drawn.push({ key: `tile-${i}`, source: s, alt: `${file}, part ${i + 1} of ${tiles.length}`, height: css(columns, t, t.height ?? 0) })
      }
      flush(tiles.length)
      return (
        <Box flexDirection="column">
          {v.error && <Text color="red">{v.error}</Text>}
          {drawn.map(d => <Svg key={d.key} source={d.source} alt={d.alt} width={columns * MDD_COLUMN_PX} height={d.height} />)}
        </Box>
      )
    }
    if (e.surface !== 'terminal') return next(e)
    const { Box, Text, Image } = $.ui.resolve(e)
    const v = view(e.surface)
    const cols = Math.min(255, e.props.bodyColumns)
    if (file && cols > 0 && cols !== v.columns) start($, v, cols, false)
    if (!file) return <Text dimColor>Run /mddog {'<file.md>'} to preview a file here.</Text>
    if (v.tiles.length === 0) return v.error ? <Text color="red">{v.error}</Text> : <Text dimColor>Rendering {file}…</Text>
    const near = nearest(v, e.props.scroll.offset, e.props.scroll.bodyRows)
    return (
      <Box flexDirection="column">
        {v.error && <Text color="red">{v.error}</Text>}
        {v.tiles.map((t, i) => !near.has(i) ? <Box key={`gap-${i}`} height={t.rows} /> : (
          <Image key={`tile-${i}`} source={{ file: t.file, format: 'png' }} columns={v.columns} rows={t.rows} alt={i === 0 ? `${file} (this terminal can't show images)` : ' '} />
        ))}
      </Box>
    )
  })
}
