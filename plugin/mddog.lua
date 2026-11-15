-- :Mddog toggles mddog's preview of the Markdown buffer. See `:checkhealth mddog`.
if vim.g.loaded_mddog then
  return
end
vim.g.loaded_mddog = true

vim.api.nvim_create_user_command('Mddog', function(cmd)
  local action = cmd.args ~= '' and cmd.args or 'toggle'
  if vim.fn.has('nvim-0.12') ~= 1 then
    return vim.notify('mddog: needs Neovim 0.12 or later', vim.log.levels.ERROR)
  end
  require('mddog')[action]()
end, {
  nargs = '?',
  complete = function()
    return { 'toggle', 'open', 'close' }
  end,
  desc = 'Toggle the mddog preview',
})
