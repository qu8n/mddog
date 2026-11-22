export const PROTOCOL = 1

// A tile's picture: the sharp one at the top level, and on the desktop a small copy at 1x.
export type Picture = { file: string; width?: number; height?: number; bytes?: number }
export type Tile = Picture & { rows: number; small?: Picture }
export type Out = { protocol: typeof PROTOCOL; keep: number; tiles: Tile[] }

export const UPDATE_MDD = 'curl -fsSL https://raw.githubusercontent.com/qu8n/mddog/main/install.sh | sh'
export const UPDATE_PLUGIN = 'claude plugin marketplace update mddog && claude plugin update mddog@mddog'

export function mismatch(protocol: unknown) {
  if (protocol === PROTOCOL) return undefined
  if (typeof protocol === 'number' && protocol > PROTOCOL) {
    return `This mddog plugin is older than mdd (protocol ${PROTOCOL}, mdd prints ${protocol}). Update the plugin with \`${UPDATE_PLUGIN}\`, then restart Claude Code.`
  }
  return `mdd is older than this mddog plugin (protocol ${PROTOCOL}, mdd prints ${typeof protocol === 'number' ? protocol : 'none'}). Update mdd with \`${UPDATE_MDD}\`, then run /mddog again.`
}
