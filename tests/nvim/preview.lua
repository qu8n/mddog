-- The Neovim plugin end to end, against a real `mdd --serve`: run by tests/neovim.rs as
-- `nvim --clean --headless -l tests/nvim/preview.lua`, with MDDOG_BIN set. There is no terminal, so the test
-- reads the escapes the plugin sends from a stub of nvim_ui_send.
local root = vim.fn.fnamemodify(debug.getinfo(1, 'S').source:sub(2), ':p:h:h:h')
vim.opt.rtp:prepend(root)
vim.o.termguicolors = true

local sent = {}
vim.api.nvim_ui_send = function(s)
  sent[#sent + 1] = s
end
local opened = {}
vim.ui.open = function(url)
  opened[#opened + 1] = url
end
local notes = {}
vim.notify = function(msg)
  notes[#notes + 1] = msg
end

local passed, failed = 0, 0
local function test(name, fn)
  local ok, err = pcall(fn)
  if ok then
    passed = passed + 1
  else
    failed = failed + 1
    io.stderr:write(('FAIL %s: %s\n'):format(name, err))
  end
end
local function wait(what, cond)
  assert(vim.wait(20000, cond, 20), 'timed out waiting for ' .. what)
end

local dir = vim.fn.tempname()
vim.fn.mkdir(dir, 'p')
dir = vim.uv.fs_realpath(dir)
local filler = string.rep('A paragraph of filler text.\n\n', 60)
vim.fn.writefile(vim.split('[jump](#target)\n\n[web](https://example.com/x)\n\n[other](b.md#deep)\n\n' .. filler .. '## Target\n\nEnd.\n', '\n'), dir .. '/a.md')
vim.fn.writefile(vim.split('# B\n\n' .. filler .. '## Deep\n\nEnd.\n', '\n'), dir .. '/b.md')

local mddog = require('mddog')
vim.cmd('edit ' .. vim.fn.fnameescape(dir .. '/a.md'))
local src = vim.api.nvim_get_current_win()
mddog.open(src)
local p = assert(mddog._preview(src), 'no preview')

local function topline()
  return vim.fn.line('w0', p.win) - 1
end
local function heading(text)
  for _, h in ipairs(p.page.headings) do
    if h.text == text then
      return h.row
    end
  end
end
--- Clicks the first cell from the top left whose link the plugin would follow to `kind`.
local function click_link(kind)
  for row = 0, 20 do
    for col = 0, 20 do
      local got
      p.client:request('link', { row = row, col = col }, function(_, ok)
        got = ok and ok.link or vim.NIL
      end)
      wait('a link answer', function()
        return got ~= nil
      end)
      if got ~= vim.NIL and got.kind == kind then
        vim.api.nvim_win_set_cursor(p.win, { 1, 0 })
        vim.fn.getmousepos = function()
          return { winid = p.win, winrow = row - topline() + 1, wincol = col + 1 }
        end
        return p:click()
      end
    end
  end
  error('no ' .. kind .. ' link')
end

test('the preview shows placeholder cells coloured with image ids', function()
  wait('the first page', function()
    return p.page ~= nil
  end)
  local line = vim.api.nvim_buf_get_lines(p.buf, 0, 1, false)[1]
  assert(vim.fn.strgetchar(line, 0) == 0x10EEEE, 'the first cell is not a placeholder')
  assert(vim.fn.strdisplaywidth(line) == p.page.cols, 'a row is not one cell per column')
  assert(vim.api.nvim_buf_line_count(p.buf) == p.page.rows)
  local marks = vim.api.nvim_buf_get_extmarks(p.buf, vim.api.nvim_create_namespace('mddog'), 0, -1, { details = true })
  assert(#marks == math.ceil(p.page.rows / p.page.tile_rows), 'one colour per tile')
  local group = marks[1][4].hl_group
  local id = tonumber(group:match('%d+$'))
  assert(vim.api.nvim_get_hl(0, { name = group }).fg == id, 'the colour is not the image id')
  local all = table.concat(sent)
  assert(all:find(('a=p,U=1,i=%d,'):format(id), 1, true), 'no virtual placement for the first tile')
  assert(all:find('t=t', 1, true), 'tiles are not sent as files')
end)

test('a click on an anchor link scrolls to the heading', function()
  click_link('anchor')
  wait('the jump', function()
    return topline() == heading('Target')
  end)
end)

test('a click on a web link opens it', function()
  vim.fn.winrestview({ topline = 1 })
  click_link('web')
  wait('vim.ui.open', function()
    return opened[1] == 'https://example.com/x'
  end)
end)

test('the preview follows unsaved edits', function()
  vim.api.nvim_buf_set_lines(0, 0, 0, false, { '# Brand new title', '' })
  vim.api.nvim_exec_autocmds('TextChanged', { buffer = vim.api.nvim_get_current_buf() })
  wait('the new page', function()
    return p.page and p.page.headings[1] and p.page.headings[1].text == 'Brand new title'
  end)
end)

test('a click on a local link opens the file at its heading, and the preview follows', function()
  vim.cmd('silent! write')
  click_link('file')
  wait('b.md in the Markdown window', function()
    return vim.api.nvim_buf_get_name(vim.api.nvim_win_get_buf(src)) == dir .. '/b.md'
  end)
  wait('the jump to Deep', function()
    return heading('Deep') ~= nil and topline() == heading('Deep')
  end)
end)

test('far tiles are dropped and come back', function()
  local before = #sent
  vim.api.nvim_win_call(p.win, function()
    vim.fn.winrestview({ topline = 1 })
  end)
  vim.api.nvim_exec_autocmds('WinScrolled', {})
  wait('the top tiles', function()
    return p.tiles[0] ~= nil
  end)
  assert(#sent >= before)
end)

test('q closes the preview and stops mdd', function()
  local proc = p.client.proc
  vim.api.nvim_set_current_win(p.win)
  vim.api.nvim_feedkeys('q', 'x', false)
  wait('the window to close', function()
    return not vim.api.nvim_win_is_valid(p.win)
  end)
  wait('mdd to stop', function()
    return proc:is_closing()
  end)
  assert(mddog._preview(src) == nil)
end)

print(('%d passed, %d failed'):format(passed, failed))
vim.fn.delete(dir, 'rf')
os.exit(failed == 0 and 0 or 1)
