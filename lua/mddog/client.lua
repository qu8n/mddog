--- Talks to `mdd --serve`: JSON lines on its standard input and output. See src/serve.rs for the protocol.
local M = {}

--- The protocol this plugin speaks. `hello` fails when mdd speaks another one.
M.PROTOCOL = 1

---@class mddog.Client
---@field private proc vim.SystemObj
---@field private next_id integer
---@field private waiting table<integer, fun(err: string?, ok: table?)>
---@field private closed boolean
local Client = {}
Client.__index = Client

--- Starts `cmd --serve`.
---@param cmd string[]
---@param on_event fun(ev: table) called with each `page` event
---@param on_exit fun(code: integer, stderr: string) called when mdd stops on its own
---@return mddog.Client
function M.start(cmd, on_event, on_exit)
  local self = setmetatable({ next_id = 0, waiting = {}, closed = false }, Client)
  local pending, stderr = '', {}
  local function on_line(line)
    local ok, msg = pcall(vim.json.decode, line)
    if not ok or type(msg) ~= 'table' then
      return
    end
    if msg.event then
      return on_event(msg)
    end
    local cb = self.waiting[msg.id]
    self.waiting[msg.id] = nil
    if cb then
      cb(msg.error, msg.ok)
    end
  end
  self.proc = vim.system(vim.list_extend(vim.list_slice(cmd), { '--serve' }), {
    stdin = true,
    stdout = function(_, data)
      if not data then
        return
      end
      -- A page event can be hundreds of kilobytes and arrive in many chunks. A plain find of each newline keeps
      -- that linear; a pattern that matches a whole line rescans the chunk from every byte when it has none.
      pending = pending .. data
      local lines, from = {}, 1
      while true do
        local nl = pending:find('\n', from, true)
        if not nl then
          break
        end
        lines[#lines + 1] = pending:sub(from, nl - 1)
        from = nl + 1
      end
      pending = pending:sub(from)
      vim.schedule(function()
        for _, l in ipairs(lines) do
          on_line(l)
        end
      end)
    end,
    stderr = function(_, data)
      stderr[#stderr + 1] = data
    end,
  }, function(out)
    vim.schedule(function()
      if not self.closed then
        self.closed = true
        on_exit(out.code, vim.trim(table.concat(stderr)))
      end
    end)
  end)
  return self
end

--- Sends a request. `cb` gets the error or the answer.
---@param cmd string
---@param fields table?
---@param cb fun(err: string?, ok: table?)?
function Client:request(cmd, fields, cb)
  if self.closed then
    return
  end
  self.next_id = self.next_id + 1
  local msg = vim.tbl_extend('force', fields or {}, { id = self.next_id, cmd = cmd })
  self.waiting[self.next_id] = cb
  self.proc:write(vim.json.encode(msg) .. '\n')
end

--- Stops mdd. Closing its standard input ends it the same way a host that quits does.
function Client:stop()
  if self.closed then
    return
  end
  self.closed = true
  pcall(self.proc.write, self.proc, nil)
  vim.defer_fn(function()
    if not self.proc:is_closing() then
      self.proc:kill(15)
    end
  end, 1000)
end

return M
