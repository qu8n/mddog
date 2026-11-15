local M = {}

function M.check()
  local h = vim.health
  h.start('mddog')
  if vim.fn.has('nvim-0.12') == 1 then
    h.ok('Neovim ' .. tostring(vim.version()))
  else
    h.error('mddog needs Neovim 0.12 or later, for nvim_ui_send()')
  end

  local _, cmd = require('mddog').config()
  if vim.fn.executable(cmd[1]) ~= 1 then
    h.error(cmd[1] .. ' not found', { 'brew install qu8n/tap/mddog', "or set bin in require('mddog').setup()" })
  else
    local proc = vim.system(vim.list_extend(vim.list_slice(cmd), { '--serve' }), { stdin = true, text = true })
    proc:write(vim.json.encode({ id = 1, cmd = 'hello', protocol = require('mddog.client').PROTOCOL }) .. '\n')
    proc:write(nil)
    local out = proc:wait(120000)
    local ok, msg = pcall(vim.json.decode, (out.stdout or ''):match('[^\n]+') or '')
    if ok and type(msg) == 'table' and msg.ok then
      h.ok(('mdd %s, protocol %d (%s)'):format(msg.ok.version, msg.ok.protocol, table.concat(cmd, ' ')))
    elseif ok and type(msg) == 'table' and msg.error then
      h.error(msg.error)
    else
      h.error(table.concat(cmd, ' ') .. ' has no --serve mode. Update mdd', { vim.trim(out.stderr or '') })
    end
  end

  local tty = vim.iter(vim.api.nvim_list_uis()):any(function(ui)
    return ui.stdout_tty
  end)
  if tty then
    h.ok('running in a terminal')
  else
    h.error('no terminal UI: GUIs such as Neovide cannot show Kitty images')
  end
  if vim.o.termguicolors then
    h.ok('termguicolors is on')
  else
    h.error('termguicolors is off: the image ids travel in 24-bit colours', { 'set termguicolors' })
  end

  local term = vim.env.TERM_PROGRAM or ''
  if term == 'WezTerm' then
    h.error('WezTerm has no Kitty Unicode placeholders, which the preview draws with')
  elseif term == 'iTerm.app' then
    h.warn('iTerm2 may not support Kitty Unicode placeholders')
  elseif term ~= '' then
    h.info('terminal: ' .. term .. '. Ghostty, kitty and Rex support Kitty Unicode placeholders')
  end
  if vim.env.TMUX then
    h.warn('inside tmux: needs `set -g allow-passthrough on`, and is untested')
  end
  if vim.env.SSH_CONNECTION or vim.env.SSH_TTY then
    h.info('over SSH: tiles travel inline as PNG, which is slower')
  end
end

return M
