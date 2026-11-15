--- mddog's Markdown preview inside Neovim. `mdd --serve` renders the page into tiles, and this plugin shows them
--- with Kitty's Unicode placeholders: the preview buffer holds placeholder text, so Neovim itself scrolls the page,
--- clips it to the window, and lets floats cover it.
---
--- The terminal keeps only so many image bytes, so the plugin loads the tiles near the view and drops the far ones.
--- Each page revision uses the other half of the preview's image ids, so the old page stays until the new one's
--- tiles are in, and an edit doesn't flash.
local api = vim.api
local client = require('mddog.client')
local placeholder = require('mddog.placeholder')

local M = {}

---@class mddog.Config
---@field bin? string|string[] the mdd command: a path, or a list such as a `cargo run` of a checkout, else
--- $MDDOG_BIN, else `mdd`
---@field sync? boolean scroll the preview to follow the cursor in the Markdown window
---@field width? number the preview's share of the Markdown window, from 0 to 1
---@field debounce? integer milliseconds after a change before the preview follows it
local config = { sync = true, width = 0.5, debounce = 150 }

--- Image ids live in 24 bits, the foreground colour that names them. Each preview takes a block of 2 x 4096 ids
--- from here, half for even revisions and half for odd ones, so nothing clashes with vim.ui.img's ids.
local ID_BASE = 0x400000
local SLOT_IDS = 0x2000
local MAX_SLOTS = 32

---@type table<integer, mddog.Preview> by the Markdown window
local previews = {}
---@type table<integer, boolean>
local slots_used = {}

local ns = api.nvim_create_namespace('mddog')

--- Writes to the terminal, through tmux when Neovim runs inside it.
local function send(s)
  if vim.env.TMUX then
    s = '\027Ptmux;' .. s:gsub('\027', '\027\027') .. '\027\\'
  end
  api.nvim_ui_send(s)
end

local function apc(control, payload)
  return '\027_G' .. control .. (payload and (';' .. payload) or '') .. '\027\\'
end

--- The mdd command, as a list.
---@return string[]
local function bin()
  local b = config.bin or vim.env.MDDOG_BIN or 'mdd'
  return type(b) == 'table' and b or { b }
end

--- A terminal on another machine can't read this machine's files, so tiles then travel inline as PNG.
local function remote()
  return vim.env.SSH_CONNECTION ~= nil or vim.env.SSH_TTY ~= nil
end

local function define_highlights()
  -- Groups that change a cell's foreground would change the image id it names, so the preview window maps them
  -- to this empty group.
  api.nvim_set_hl(0, 'MddogNone', {})
  -- A cursor whose highlight has blend=100 is hidden by the TUI.
  api.nvim_set_hl(0, 'MddogCursor', { blend = 100, nocombine = true })
end

---@class mddog.Page
---@field rev integer
---@field cols integer
---@field rows integer
---@field tile_rows integer
---@field headings {level: integer, text: string, row: integer}[]
---@field blocks {start: integer, end: integer, row: integer, rows: integer}[]

---@class mddog.Tile
---@field id integer
---@field bytes integer

---@class mddog.Preview
---@field src_win integer
---@field src_buf integer
---@field win integer
---@field buf integer
---@field slot integer
---@field client mddog.Client
---@field budget integer
---@field page? mddog.Page the page the buffer shows
---@field next? mddog.Page a newer page whose first tiles are on the way
---@field tiles table<integer, mddog.Tile> the shown page's tiles in the terminal, by index
---@field asked table<integer, boolean> the shown page's tiles requested
---@field lines_for? string the geometry of the buffer's text
---@field heading? string a heading to jump to once the next page shows
---@field timer? uv.uv_timer_t
---@field group integer
local Preview = {}
Preview.__index = Preview

function Preview:id(index, rev)
  return ID_BASE + self.slot * SLOT_IDS + (rev % 2) * (SLOT_IDS / 2) + index
end

function Preview:visible_rows()
  if not api.nvim_win_is_valid(self.win) then
    return 0, 0
  end
  local top = vim.fn.line('w0', self.win) - 1
  return top, top + api.nvim_win_get_height(self.win)
end

--- The tiles of `page` within one window height of the view.
function Preview:wanted(page)
  local top, bottom = self:visible_rows()
  local h = bottom - top
  local first = math.max(0, math.floor((top - h) / page.tile_rows))
  local last = math.min(math.ceil(page.rows / page.tile_rows) - 1, math.floor((bottom + h) / page.tile_rows))
  local out = {}
  for i = first, last do
    out[#out + 1] = i
  end
  return out
end

--- Sends tiles to the terminal and gives each a virtual placement that its placeholder cells show.
function Preview:transmit(page, tiles)
  local out = {}
  for _, t in ipairs(tiles) do
    local id = self:id(t.index, page.rev)
    if t.format == 'png' then
      local f = io.open(t.file, 'rb')
      local data = f and f:read('*a') or ''
      if f then
        f:close()
      end
      os.remove(t.file)
      local b64 = vim.base64.encode(data)
      for pos = 1, #b64, 4096 do
        local more = pos + 4096 <= #b64 and 1 or 0
        local control = pos == 1 and ('a=t,f=100,t=d,i=%d,q=2,m=%d'):format(id, more) or ('m=%d'):format(more)
        out[#out + 1] = apc(control, b64:sub(pos, pos + 4095))
      end
    else
      -- t=t: the terminal reads the file, then deletes it.
      out[#out + 1] = apc(('a=t,f=32,s=%d,v=%d,t=t,i=%d,q=2'):format(t.width, t.height, id), vim.base64.encode(t.file))
    end
    local rows = math.min(page.tile_rows, page.rows - t.index * page.tile_rows)
    out[#out + 1] = apc(('a=p,U=1,i=%d,c=%d,r=%d,q=2'):format(id, page.cols, rows))
    api.nvim_set_hl(0, 'MddogImage' .. id, { fg = id })
  end
  send(table.concat(out))
end

--- Removes images from the terminal.
local function drop(ids)
  local out = {}
  for _, id in ipairs(ids) do
    out[#out + 1] = apc(('a=d,d=I,i=%d,q=2'):format(id))
  end
  if #out > 0 then
    send(table.concat(out))
  end
end

--- Shows `page`: its placeholder text and colours, then drops the old page's images.
function Preview:commit(page, tiles)
  if not api.nvim_buf_is_valid(self.buf) then
    return
  end
  -- Two revisions apart, a page reuses the same ids, and sending a tile again replaced the old image in place.
  local fresh = {}
  for _, t in ipairs(tiles) do
    fresh[self:id(t.index, page.rev)] = true
  end
  local old = {}
  for _, t in pairs(self.tiles) do
    if not fresh[t.id] then
      old[#old + 1] = t.id
    end
  end
  local geometry = ('%d:%d:%d'):format(page.cols, page.rows, page.tile_rows)
  vim.bo[self.buf].modifiable = true
  if geometry ~= self.lines_for then
    local rows = {}
    for r = 0, page.tile_rows - 1 do
      rows[r] = placeholder.row(r, page.cols)
    end
    local lines = {}
    for r = 0, page.rows - 1 do
      lines[r + 1] = rows[r % page.tile_rows]
    end
    api.nvim_buf_set_lines(self.buf, 0, -1, false, lines)
    self.lines_for = geometry
  end
  vim.bo[self.buf].modifiable = false
  api.nvim_buf_clear_namespace(self.buf, ns, 0, -1)
  for i = 0, math.ceil(page.rows / page.tile_rows) - 1 do
    local first = i * page.tile_rows
    api.nvim_buf_set_extmark(self.buf, ns, first, 0, {
      end_row = math.min(first + page.tile_rows, page.rows),
      end_col = 0,
      hl_group = 'MddogImage' .. self:id(i, page.rev),
      priority = 65535,
      strict = false,
    })
  end
  self.page, self.tiles, self.asked = page, {}, {}
  for _, t in ipairs(tiles) do
    self.tiles[t.index] = { id = self:id(t.index, page.rev), bytes = t.width * t.height * 4 }
    self.asked[t.index] = true
  end
  drop(old)
  if self.heading then
    local id = self.heading
    self.heading = nil
    self.client:request('anchor', { anchor = id }, function(_, ok)
      if ok and ok.row ~= vim.NIL then
        self:scroll_to(ok.row)
      end
    end)
  else
    self:sync()
  end
  self:fill()
end

--- A new page from mdd: fetch the tiles in view, and show it once they are in.
function Preview:on_page(page)
  self.next = page
  local want = self:wanted(page)
  self.client:request('tiles', { rev = page.rev, tiles = want }, function(err, ok)
    if err or self.next ~= page then
      return
    end
    self.next = nil
    self:transmit(page, ok.tiles)
    self:commit(page, ok.tiles)
  end)
end

--- Loads the shown page's tiles near the view, and drops far ones while the terminal holds too many bytes.
function Preview:fill()
  local page = self.page
  if not page or self.next then
    return
  end
  local missing = {}
  for _, i in ipairs(self:wanted(page)) do
    if not self.asked[i] then
      self.asked[i] = true
      missing[#missing + 1] = i
    end
  end
  if #missing > 0 then
    self.client:request('tiles', { rev = page.rev, tiles = missing }, function(err, ok)
      if err or self.page ~= page or self.next then
        return
      end
      self:transmit(page, ok.tiles)
      for _, t in ipairs(ok.tiles) do
        self.tiles[t.index] = { id = self:id(t.index, page.rev), bytes = t.width * t.height * 4 }
      end
      self:evict()
    end)
  end
end

function Preview:evict()
  local total = 0
  for _, t in pairs(self.tiles) do
    total = total + t.bytes
  end
  -- Other previews and other image plugins share the terminal's storage, so keep well under it.
  local limit = math.floor(self.budget / 4)
  if total <= limit then
    return
  end
  local top, bottom = self:visible_rows()
  local centre = (top + bottom) / 2 / self.page.tile_rows
  local order = vim.tbl_keys(self.tiles)
  table.sort(order, function(a, b)
    return math.abs(a - centre) > math.abs(b - centre)
  end)
  local gone = {}
  for _, i in ipairs(order) do
    local first = i * self.page.tile_rows
    local seen = first < bottom and first + self.page.tile_rows > top
    if total <= limit / 2 or seen then
      break
    end
    total = total - self.tiles[i].bytes
    gone[#gone + 1] = self.tiles[i].id
    self.tiles[i], self.asked[i] = nil, nil
  end
  drop(gone)
end

function Preview:scroll_to(row)
  if api.nvim_win_is_valid(self.win) then
    api.nvim_win_call(self.win, function()
      vim.fn.winrestview({ topline = math.max(1, row + 1), lnum = math.max(1, row + 1) })
    end)
  end
end

--- The page row where source line `line` shows: within its block by the share of the block's lines above it, and
--- at the next block for a blank line between blocks.
local function row_of_line(page, line)
  for _, b in ipairs(page.blocks) do
    if line <= b['end'] then
      if line < b.start then
        return b.row
      end
      local span = math.max(1, b['end'] - b.start + 1)
      return b.row + math.floor((line - b.start) / span * b.rows)
    end
  end
  return page.rows
end

--- Scrolls the preview so its top shows what the top of the Markdown window shows.
function Preview:sync()
  local page = self.page
  if not config.sync or not page or not api.nvim_win_is_valid(self.src_win) or api.nvim_win_get_buf(self.src_win) ~= self.src_buf then
    return
  end
  local top = vim.fn.line('w0', self.src_win)
  self:scroll_to(top <= 1 and 0 or row_of_line(page, top))
end

function Preview:send_text()
  if not api.nvim_buf_is_valid(self.src_buf) then
    return
  end
  local text = table.concat(api.nvim_buf_get_lines(self.src_buf, 0, -1, false), '\n')
  self.client:request('open', { text = text, path = api.nvim_buf_get_name(self.src_buf) })
end

function Preview:schedule_text()
  if self.timer then
    self.timer:stop()
  else
    self.timer = assert(vim.uv.new_timer())
  end
  self.timer:start(config.debounce, 0, vim.schedule_wrap(function()
    self:send_text()
  end))
end

function Preview:send_size()
  if api.nvim_win_is_valid(self.win) then
    self.client:request('size', { cols = math.min(api.nvim_win_get_width(self.win), placeholder.MAX) })
  end
end

--- Follows the link under the mouse, else presses what is there, such as a copy button or a <details> summary.
function Preview:click()
  local m = vim.fn.getmousepos()
  if m.winid ~= self.win or not self.page then
    return
  end
  local row = vim.fn.line('w0', self.win) - 1 + m.winrow - 1
  local col = m.wincol - 1
  self.client:request('link', { row = row, col = col }, function(_, ok)
    local link = ok and ok.link
    if not link or link == vim.NIL then
      return self.client:request('press', { row = row, col = col }, function(_, pressed)
        if pressed and pressed.copy then
          vim.fn.setreg('+', pressed.copy)
          vim.notify('mddog: copied')
        end
      end)
    end
    if link.kind == 'web' then
      vim.ui.open(link.url)
    elseif link.kind == 'anchor' then
      self:scroll_to(link.row)
    elseif link.kind == 'file' then
      if api.nvim_win_is_valid(self.src_win) then
        self.heading = link.heading ~= vim.NIL and link.heading or nil
        api.nvim_win_call(self.src_win, function()
          vim.cmd.edit(vim.fn.fnameescape(link.path))
        end)
      end
    else
      vim.notify('mddog: ' .. (link.why or 'not followed'), vim.log.levels.WARN)
    end
  end)
end

--- Jumps to the next (`dir` 1) or previous (-1) heading.
function Preview:heading_jump(dir)
  if not self.page then
    return
  end
  local top = vim.fn.line('w0', self.win) - 1
  local list = self.page.headings
  for k = dir > 0 and 1 or #list, dir > 0 and #list or 1, dir do
    local h = list[k]
    if (dir > 0 and h.row > top) or (dir < 0 and h.row < top) then
      return self:scroll_to(h.row)
    end
  end
end

function Preview:close()
  if previews[self.src_win] ~= self then
    return
  end
  previews[self.src_win] = nil
  slots_used[self.slot] = nil
  pcall(api.nvim_del_augroup_by_id, self.group)
  if self.timer then
    self.timer:stop()
    self.timer:close()
  end
  self.client:stop()
  local ids = {}
  for _, t in pairs(self.tiles) do
    ids[#ids + 1] = t.id
  end
  drop(ids)
  if api.nvim_win_is_valid(self.win) and #api.nvim_list_wins() > 1 then
    api.nvim_win_close(self.win, true)
  end
  if api.nvim_buf_is_valid(self.buf) then
    api.nvim_buf_delete(self.buf, { force = true })
  end
end

local function window_options(win)
  local wo = vim.wo[win]
  wo.number, wo.relativenumber, wo.signcolumn, wo.foldcolumn = false, false, 'no', '0'
  wo.foldenable, wo.spell, wo.wrap, wo.list = false, false, false, false
  wo.cursorline, wo.cursorcolumn, wo.colorcolumn, wo.statuscolumn = false, false, '', ''
  wo.conceallevel = 0
  wo.winhighlight = table.concat(vim.tbl_map(function(g)
    return g .. ':MddogNone'
  end, { 'Visual', 'VisualNOS', 'Search', 'CurSearch', 'IncSearch', 'Substitute', 'MatchParen', 'CursorLine', 'CursorColumn', 'SpellBad', 'SpellCap', 'SpellRare', 'SpellLocal' }), ',')
end

--- Placeholder cells must be one column wide, which `ambiwidth=double` would undo.
local function one_cell()
  if vim.o.ambiwidth ~= 'double' then
    return
  end
  local widths = vim.fn.getcellwidths()
  for _, w in ipairs(widths) do
    if w[1] <= placeholder.CHAR and w[2] >= placeholder.CHAR then
      return
    end
  end
  widths[#widths + 1] = { placeholder.CHAR, placeholder.CHAR, 1 }
  table.sort(widths, function(a, b)
    return a[1] < b[1]
  end)
  vim.fn.setcellwidths(widths)
end

--- Opens a preview of the Markdown buffer in window `win`.
---@param win? integer
function M.open(win)
  win = win or api.nvim_get_current_win()
  if previews[win] then
    return
  end
  if vim.fn.executable(bin()[1]) ~= 1 then
    return vim.notify(('mddog: %s not found. Install it with `brew install qu8n/tap/mddog`, or set bin in setup()'):format(bin()[1]), vim.log.levels.ERROR)
  end
  local slot
  for s = 0, MAX_SLOTS - 1 do
    if not slots_used[s] then
      slot = s
      break
    end
  end
  if not slot then
    return vim.notify('mddog: too many previews open', vim.log.levels.ERROR)
  end
  define_highlights()
  one_cell()
  local buf = api.nvim_create_buf(false, true)
  vim.bo[buf].bufhidden = 'wipe'
  vim.bo[buf].filetype = 'mddog'
  local src_buf = api.nvim_win_get_buf(win)
  pcall(api.nvim_buf_set_name, buf, 'mddog://' .. vim.fn.fnamemodify(api.nvim_buf_get_name(src_buf), ':t'))
  local width = math.max(20, math.floor(api.nvim_win_get_width(win) * config.width))
  local pwin = api.nvim_open_win(buf, false, { split = 'right', win = win, width = width })
  window_options(pwin)

  ---@type mddog.Preview
  local self = setmetatable({
    src_win = win,
    src_buf = src_buf,
    win = pwin,
    buf = buf,
    slot = slot,
    budget = 300 * 1024 * 1024,
    tiles = {},
    asked = {},
    group = api.nvim_create_augroup('mddog_preview_' .. pwin, {}),
  }, Preview)
  previews[win] = self
  slots_used[slot] = true

  self.client = client.start(bin(), function(ev)
    if ev.event == 'page' then
      self:on_page(ev)
    end
  end, function(code, stderr)
    vim.notify(('mddog: mdd stopped (%d)%s'):format(code, stderr ~= '' and (': ' .. stderr) or ''), vim.log.levels.ERROR)
    self:close()
  end)
  self.client:request('hello', { protocol = client.PROTOCOL, png = remote(), p3 = vim.env.TERM_PROGRAM == 'ghostty' }, function(err, ok)
    if err then
      vim.notify('mddog: ' .. err, vim.log.levels.ERROR)
      return self:close()
    end
    self.budget = ok.budget or self.budget
    self:send_size()
    self:send_text()
  end)

  local g = self.group
  api.nvim_create_autocmd({ 'TextChanged', 'TextChangedI' }, {
    group = g,
    callback = function(ev)
      if ev.buf == self.src_buf then
        self:schedule_text()
      end
    end,
  })
  -- The preview follows the Markdown window to another Markdown file, such as one a link opened.
  api.nvim_create_autocmd('BufWinEnter', {
    group = g,
    callback = function(ev)
      if api.nvim_get_current_win() == self.src_win and ev.buf ~= self.src_buf and vim.bo[ev.buf].filetype == 'markdown' then
        self.src_buf = ev.buf
        pcall(api.nvim_buf_set_name, self.buf, 'mddog://' .. vim.fn.fnamemodify(api.nvim_buf_get_name(ev.buf), ':t'))
        self:send_text()
      end
    end,
  })
  api.nvim_create_autocmd('WinScrolled', {
    group = g,
    callback = function()
      -- v:event is gone once the callback returns.
      local source = vim.v.event[tostring(self.src_win)] ~= nil
      vim.schedule(function()
        if source then
          self:sync()
        end
        self:fill()
      end)
    end,
  })
  api.nvim_create_autocmd({ 'WinResized', 'VimResized' }, {
    group = g,
    callback = function(ev)
      -- A font zoom changes the terminal's size in cells, and mdd measures the new cell when asked for a size.
      if ev.event == 'VimResized' or vim.tbl_contains(vim.v.event.windows or {}, self.win) then
        self:send_size()
      end
    end,
  })
  api.nvim_create_autocmd({ 'ColorScheme', 'OptionSet' }, {
    group = g,
    pattern = { '*', 'background' },
    callback = function(ev)
      if ev.event == 'OptionSet' and ev.match ~= 'background' then
        return
      end
      define_highlights()
      for _, t in pairs(self.tiles) do
        api.nvim_set_hl(0, 'MddogImage' .. t.id, { fg = t.id })
      end
      -- mdd picks light or dark from the system each time it lays a page out.
      self:send_text()
    end,
  })
  api.nvim_create_autocmd('BufEnter', {
    group = g,
    buffer = buf,
    callback = function()
      self.cursor = vim.o.guicursor
      vim.o.guicursor = 'a:MddogCursor'
    end,
  })
  api.nvim_create_autocmd('BufLeave', {
    group = g,
    buffer = buf,
    callback = function()
      if self.cursor then
        vim.o.guicursor, self.cursor = self.cursor, nil
      end
    end,
  })
  api.nvim_create_autocmd('WinClosed', {
    group = g,
    callback = function(ev)
      local closed = tonumber(ev.match)
      if closed == self.win or closed == self.src_win then
        vim.schedule(function()
          self:close()
        end)
      end
    end,
  })
  api.nvim_create_autocmd('VimLeavePre', {
    group = g,
    callback = function()
      self:close()
    end,
  })

  local map = function(lhs, fn, desc)
    vim.keymap.set('n', lhs, fn, { buffer = buf, desc = desc, nowait = true })
  end
  map('q', function()
    self:close()
  end, 'Close the mddog preview')
  map(']]', function()
    self:heading_jump(1)
  end, 'Next heading')
  map('[[', function()
    self:heading_jump(-1)
  end, 'Previous heading')
  map('<LeftRelease>', function()
    self:click()
  end, 'Follow a link, or press a button')
end

--- Closes the preview of window `win`, or the preview that `win` is.
---@param win? integer
function M.close(win)
  win = win or api.nvim_get_current_win()
  for src, p in pairs(previews) do
    if src == win or p.win == win then
      return p:close()
    end
  end
end

---@param win? integer
function M.toggle(win)
  win = win or api.nvim_get_current_win()
  for src, p in pairs(previews) do
    if src == win or p.win == win then
      return p:close()
    end
  end
  M.open(win)
end

---@param opts? mddog.Config
function M.setup(opts)
  config = vim.tbl_extend('force', config, opts or {})
end

--- The preview of Markdown window `win`, for the tests.
---@private
function M._preview(win)
  return previews[win]
end

--- For :checkhealth.
function M.config()
  return config, bin()
end

return M
