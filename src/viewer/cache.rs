use crate::render::engine::Snapshot;
use crate::term::kitty::Shm;

/// A tile drawn into shared memory, ready to send.
pub type Pixels = Snapshot<Shm>;
use crate::term::kitty::{self, Medium, Place};

const MAX_BANDS: usize = 4;

struct Tile {
    top: u32,
    height: u32,
    /// The rows sent below `height`: one cell, so a view that starts between cells can draw its row across the
    /// seam from this tile alone. None for the last tile.
    post: u32,
    id: u32,
    bytes: usize,
    resident: bool,
    stale: Vec<u32>,
    dirty: Vec<(u32, u32)>,
    bands: Vec<Band>,
}

struct Band {
    top: u32,
    height: u32,
    /// Where the sent image starts: a cell above `top`, for the same reason as `Tile::post`.
    src: u32,
    id: u32,
    bytes: usize,
}

impl Tile {
    fn new(top: u32, height: u32, post: u32) -> Tile {
        Tile { top, height, post, id: 0, bytes: 0, resident: false, stale: vec![], dirty: vec![], bands: vec![] }
    }

    fn release(&mut self) -> (Vec<u32>, usize) {
        let ids = self.resident.then_some(self.id).into_iter().chain(self.bands.drain(..).map(|b| b.id)).collect();
        let bytes = if self.resident { self.bytes } else { 0 };
        self.resident = false;
        self.dirty.clear();
        (ids, bytes)
    }
}

pub struct View {
    pub scroll_px: u32,
    pub bottom: u32,
    pub cell_h: u32,
    pub col: u16,
    pub cols: u16,
    pub width: u32,
}

pub struct Batch<'a> {
    pub keep: &'a [usize],
    pub scroll_px: u32,
    pub snap: &'a mut dyn FnMut(u32, u32) -> Option<Pixels>,
    pub bytes: Vec<u8>,
    pub cmds: String,
}

pub struct Cache {
    tiles: Vec<Tile>,
    placed: Vec<u32>,
    doomed: Vec<u32>,
    next_id: u32,
    seq: u64,
    cell_h: u32,
    total: u32,
    resident: usize,
    storage_limit: usize,
    bands: bool,
    medium: Medium,
    /// `MDDOG_DUMP_TILES`: a folder each sent tile is also written to, as PNG.
    dump: Option<std::path::PathBuf>,
}

impl Cache {
    pub fn new(storage_limit: usize, bands: bool, medium: Medium, first_id: u32) -> Cache {
        Cache {
            tiles: vec![],
            placed: vec![],
            doomed: vec![],
            next_id: first_id,
            seq: 0,
            cell_h: 1,
            total: 0,
            resident: 0,
            storage_limit,
            bands,
            medium,
            dump: std::env::var_os("MDDOG_DUMP_TILES").map(Into::into),
        }
    }

    pub fn bands(&self) -> bool {
        self.bands
    }

    pub fn resident(&self) -> usize {
        self.resident
    }

    pub fn placed(&self) -> &[u32] {
        &self.placed
    }

    pub fn held(&self) -> usize {
        self.tiles.iter().filter(|t| t.resident).map(|t| t.bytes).sum()
    }

    /// Cuts the page into tiles of `tile_h`, each sent with one more cell row below it (see `Tile::post`).
    pub fn retile(&mut self, out: &mut String, top: u32, total: u32, tile_h: u32, cell_h: u32) -> u32 {
        for t in &mut self.tiles {
            for id in t.release().0.into_iter().chain(t.stale.drain(..)) {
                if self.placed.contains(&id) {
                    self.doomed.push(id);
                } else {
                    kitty::free(out, id);
                }
            }
        }
        self.placed.clear();
        self.tiles.clear();
        self.resident = 0;
        (self.cell_h, self.total) = (cell_h, total);
        let mut top = top;
        while top < total {
            let height = tile_h.min(total - top);
            self.tiles.push(Tile::new(top, height, cell_h.min(total - top - height)));
            top += height;
        }
        top
    }

    pub fn invalidate(&mut self, y0: u32, y1: u32) {
        for t in &mut self.tiles {
            if t.resident && t.top < y1 && t.top + t.height + t.post > y0 {
                if self.bands {
                    t.dirty.push((y0.max(t.top), y1.min(t.top + t.height + t.post)));
                } else {
                    let (ids, bytes) = t.release();
                    t.stale.extend(ids);
                    self.resident -= bytes;
                }
            }
        }
    }

    pub fn visible(&self, v: &View) -> Vec<usize> {
        (0..self.tiles.len())
            .filter(|&i| {
                let (r0, r1) = rows(v, self.tiles[i].top, self.tiles[i].top + self.tiles[i].height, false);
                r0 < r1
            })
            .collect()
    }

    pub fn is_resident(&self, i: usize) -> bool {
        self.tiles[i].resident
    }

    pub fn has_bands(&self) -> bool {
        self.tiles.iter().any(|t| !t.bands.is_empty())
    }

    pub fn fold(&mut self) {
        for t in &mut self.tiles {
            if !t.bands.is_empty() {
                let (ids, bytes) = t.release();
                t.stale.extend(ids);
                self.resident -= bytes;
            }
        }
    }

    pub fn plan_bands(&mut self, vis: &[usize], idle: bool, phase: u32, cell_h: u32, frees: &mut String) -> Vec<(usize, u32, u32)> {
        let mut todo = Vec::new();
        for (i, t) in self.tiles.iter_mut().enumerate() {
            if t.dirty.is_empty() {
                continue;
            }
            let end = t.top + t.height + t.post;
            let dirty: Vec<(u32, u32)> = t.dirty.drain(..).map(|(a, b)| (phase + (a - phase) / cell_h * cell_h, (phase + (b - phase).div_ceil(cell_h) * cell_h).min(end))).collect();
            let old: Vec<(u32, u32)> = t.bands.iter().map(|b| (b.top, b.top + b.height)).collect();
            let (spans, gone) = join_bands(&dirty, &old);
            let kept = (0..old.len()).filter(|k| !gone.contains(k));
            let rows: u32 = kept.clone().map(|k| old[k].1 - old[k].0).chain(spans.iter().map(|s| s.1 - s.0)).sum();
            if idle || !vis.contains(&i) || kept.count() + spans.len() > MAX_BANDS || rows * 2 > t.height {
                let (ids, bytes) = t.release();
                t.stale.extend(ids);
                self.resident -= bytes;
                continue;
            }
            for k in gone.into_iter().rev() {
                let b = t.bands.remove(k);
                kitty::free(frees, b.id);
                self.placed.retain(|&p| p != b.id);
                t.bytes -= b.bytes;
                self.resident -= b.bytes;
            }
            todo.extend(spans.into_iter().map(|(a, b)| (i, a, b - a)));
        }
        todo
    }

    pub fn free_stale(&mut self, frees: &mut String) {
        for t in &mut self.tiles {
            for s in t.stale.drain(..) {
                kitty::free(frees, s);
                self.placed.retain(|&id| id != s);
            }
        }
        for id in self.doomed.drain(..) {
            kitty::free(frees, id);
        }
    }

    pub fn free_all(&mut self, out: &mut String) {
        for t in &mut self.tiles {
            for id in t.release().0 {
                kitty::free(out, id);
            }
        }
        for id in self.doomed.drain(..) {
            kitty::free(out, id);
        }
        self.placed.clear();
        self.resident = 0;
    }

    pub fn upload(&mut self, i: usize, b: &mut Batch) -> bool {
        let (top, height) = (self.tiles[i].top, self.tiles[i].height + self.tiles[i].post);
        let Some((id, need)) = self.send(top, height, "", b) else { return false };
        let t = &mut self.tiles[i];
        t.id = id;
        t.bytes = need;
        t.resident = true;
        self.resident += need;
        true
    }

    pub fn upload_band(&mut self, i: usize, top: u32, height: u32, b: &mut Batch) {
        let src = top.saturating_sub(self.cell_h);
        let end = (top + height + self.cell_h).min(self.total.max(top + height));
        let Some((id, need)) = self.send(src, end - src, "band-", b) else {
            let (ids, gone) = self.tiles[i].release();
            self.tiles[i].stale.extend(ids);
            self.resident -= gone;
            self.upload(i, b);
            return;
        };
        let t = &mut self.tiles[i];
        t.bands.push(Band { top, height, src, id, bytes: need });
        t.bytes += need;
        self.resident += need;
    }

    fn send(&mut self, top: u32, height: u32, tag: &str, b: &mut Batch) -> Option<(u32, usize)> {
        let bmp = (b.snap)(top, height)?;
        let need = bmp.rgba.len();
        self.evict_for(need, b.keep, b.scroll_px, &mut b.cmds);
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.seq += 1;
        if let Some(dir) = &self.dump
            && let Some(png) = kitty::png(bmp.width, bmp.height, &bmp.rgba)
        {
            let _ = std::fs::write(dir.join(format!("{tag}{id}-{top}.png")), png);
        }
        self.medium = kitty::transmit(&mut b.bytes, self.medium, id, bmp.width, bmp.height, bmp.rgba, self.seq);
        Some((id, need))
    }

    fn evict_for(&mut self, need: usize, keep: &[usize], scroll_px: u32, out: &mut String) {
        while self.resident + need > self.storage_limit {
            let far = (0..self.tiles.len()).filter(|i| self.tiles[*i].resident && !keep.contains(i)).max_by_key(|&i| (i64::from(self.tiles[i].top) - i64::from(scroll_px)).unsigned_abs());
            let Some(i) = far else { break };
            let (ids, bytes) = self.tiles[i].release();
            for id in ids {
                kitty::free(out, id);
                self.placed.retain(|&p| p != id);
            }
            self.resident -= bytes;
        }
    }

    pub fn place(&mut self, vis: &[usize], v: &View, out: &mut String) {
        let mut placed = Vec::new();
        for &i in vis {
            let t = &self.tiles[i];
            if !t.resident {
                continue;
            }
            let tile = (t.id, t.top, rows(v, t.top, t.top + t.height, false), 0);
            let bands = t.bands.iter().map(|b| (b.id, b.src, rows(v, b.top, b.top + b.height, true), 1));
            for (id, src, (r0, r1), z) in std::iter::once(tile).chain(bands) {
                if r1 <= r0 {
                    continue;
                }
                let y = v.scroll_px + r0 * v.cell_h - src;
                kitty::place(out, &Place { id, row: r0, col: u32::from(v.col), y, width: v.width, height: (r1 - r0) * v.cell_h, cols: u32::from(v.cols), rows: r1 - r0, z });
                placed.push(id);
            }
        }
        for id in std::mem::take(&mut self.placed) {
            if !placed.contains(&id) {
                kitty::unplace(out, id);
            }
        }
        self.placed = placed;
    }

    /// Whether the tiles fit in the storage limit together, at `width` pixels wide.
    pub fn fits(&self, tiles: &[usize], width: u32) -> bool {
        let need = |t: &Tile| if t.resident { t.bytes } else { (width * (t.height + t.post) * 4) as usize };
        tiles.iter().map(|&i| need(&self.tiles[i])).sum::<usize>() <= self.storage_limit
    }

    pub fn prefetch(&self, scroll_px: u32, width: u32) -> Option<usize> {
        let tiles: Vec<(u32, bool, usize)> = self.tiles.iter().map(|t| (t.top, t.resident, (width * (t.height + t.post) * 4) as usize)).collect();
        prefetch_pick(&tiles, scroll_px, self.resident, self.storage_limit)
    }
}

/// The view's cell rows an image draws, `r0..r1`. Row `r` shows the page from `scroll_px + r * cell_h`, which need
/// not fall on a tile's cell grid while a glide is between cells. A tile draws the rows that start inside it, and
/// its extra row covers the end of the last one. A band draws every row it touches, from its extra row above.
fn rows(v: &View, y0: u32, y1: u32, touching: bool) -> (u32, u32) {
    let (ch, sp) = (i64::from(v.cell_h), i64::from(v.scroll_px));
    let n = i64::from((v.bottom - v.scroll_px) / v.cell_h);
    let a = i64::from(y0) - sp;
    let r0 = if touching { a.div_euclid(ch) } else { (a + ch - 1).div_euclid(ch) };
    let r1 = (i64::from(y1) - sp + ch - 1).div_euclid(ch);
    (r0.clamp(0, n) as u32, r1.clamp(0, n) as u32)
}

fn join_bands(dirty: &[(u32, u32)], bands: &[(u32, u32)]) -> (Vec<(u32, u32)>, Vec<usize>) {
    let mut items: Vec<(u32, u32, Option<usize>)> = dirty.iter().map(|&(a, b)| (a, b, None)).chain(bands.iter().enumerate().map(|(k, &(a, b))| (a, b, Some(k)))).collect();
    items.sort_by_key(|x| x.0);
    let (mut spans, mut gone) = (vec![], vec![]);
    let mut i = 0;
    while i < items.len() {
        let (lo, mut hi) = (items[i].0, items[i].1);
        let mut j = i + 1;
        while j < items.len() && items[j].0 <= hi {
            hi = hi.max(items[j].1);
            j += 1;
        }
        if items[i..j].iter().any(|x| x.2.is_none()) {
            spans.push((lo, hi));
            gone.extend(items[i..j].iter().filter_map(|x| x.2));
        }
        i = j;
    }
    gone.sort_unstable();
    (spans, gone)
}

fn prefetch_pick(tiles: &[(u32, bool, usize)], scroll_px: u32, resident: usize, storage_limit: usize) -> Option<usize> {
    let i = (0..tiles.len()).filter(|&i| !tiles[i].1).min_by_key(|&i| (i64::from(tiles[i].0) - i64::from(scroll_px)).unsigned_abs())?;
    (resident + tiles[i].2 <= storage_limit).then_some(i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::kitty::Shm;

    fn snap(_: u32, height: u32) -> Option<Pixels> {
        Some(Pixels { width: 1, height: height as usize, rgba: Shm::create(kitty::shm_name(), height as usize * 4)? })
    }

    fn cache(bands: bool) -> Cache {
        let mut c = Cache::new(900, bands, Medium::Direct, 1);
        assert_eq!(c.retile(&mut String::new(), 0, 400, 100, 10), 400);
        c
    }

    fn upload(c: &mut Cache, i: usize, keep: &[usize], scroll_px: u32) -> String {
        let mut b = Batch { keep, scroll_px, snap: &mut snap, bytes: Vec::new(), cmds: String::new() };
        assert!(c.upload(i, &mut b));
        b.cmds
    }

    fn view(scroll_px: u32) -> View {
        View { scroll_px, bottom: scroll_px + 100, cell_h: 10, col: 0, cols: 10, width: 1 }
    }

    #[test]
    fn the_farthest_tile_goes_first_and_the_storage_limit_holds() {
        let mut c = cache(false);
        upload(&mut c, 0, &[], 300);
        upload(&mut c, 1, &[], 300);
        let cmds = upload(&mut c, 3, &[3], 300);
        assert!(cmds.contains("i=1,"));
        assert_eq!((c.is_resident(0), c.is_resident(1), c.is_resident(3), c.resident()), (false, true, true, 840));
    }

    #[test]
    fn a_change_resends_the_tile_or_a_band_under_herdr() {
        let mut c = cache(false);
        upload(&mut c, 0, &[], 0);
        c.invalidate(45, 55);
        let mut frees = String::new();
        c.free_stale(&mut frees);
        assert!(!c.is_resident(0) && c.resident() == 0 && frees.contains("i=1,"));

        let mut c = cache(true);
        upload(&mut c, 0, &[], 0);
        c.invalidate(45, 55);
        assert_eq!(c.plan_bands(&[0], false, 0, 10, &mut String::new()), vec![(0, 40, 20)]);
        let mut b = Batch { keep: &[0], scroll_px: 0, snap: &mut snap, bytes: Vec::new(), cmds: String::new() };
        c.upload_band(0, 40, 20, &mut b);
        assert!(c.has_bands() && c.resident() == 600, "{}", c.resident());
        c.fold();
        assert!(!c.has_bands() && c.resident() == 0);
    }

    #[test]
    fn tiles_that_scroll_out_are_unplaced() {
        let mut c = cache(false);
        upload(&mut c, 0, &[], 0);
        let mut out = String::new();
        c.place(&c.visible(&view(0)), &view(0), &mut out);
        assert_eq!(c.placed(), [1]);
        out.clear();
        c.place(&c.visible(&view(200)), &view(200), &mut out);
        assert!(c.placed().is_empty() && out.contains("d=i,i=1,p=1,"));
    }

    /// Each placement as (row, id, source y, source height, rows).
    fn placements(out: &str) -> Vec<(u32, u32, u32, u32, u32)> {
        out.split("\x1b[")
            .skip(1)
            .map(|p| {
                let row: u32 = p.split(';').next().unwrap().parse().unwrap();
                let key = |k: &str| p.split(',').find_map(|f| f.strip_prefix(k)).unwrap().parse::<u32>().unwrap();
                (row - 1, key("i="), key("y="), key("h="), key("r="))
            })
            .collect()
    }

    #[test]
    fn a_view_between_cells_draws_each_row_once_at_one_to_one() {
        for ch in [10, 7] {
            let mut c = Cache::new(usize::MAX, false, Medium::Direct, 1);
            c.retile(&mut String::new(), 3, 3 + 40 * ch, 10 * ch, ch);
            for i in 0..4 {
                upload(&mut c, i, &[], 0);
            }
            for sp in 3..3 + 32 * ch {
                let v = View { scroll_px: sp, bottom: sp + 8 * ch, cell_h: ch, col: 0, cols: 10, width: 1 };
                let mut out = String::new();
                c.place(&c.visible(&v), &v, &mut out);
                let mut drawn = vec![];
                for (row, id, y, h, rows) in placements(&out) {
                    let t = &c.tiles[id as usize - 1];
                    assert_eq!(h, rows * ch, "rescaled at {sp}");
                    assert!(y + h <= t.height + t.post, "past the image at {sp}");
                    assert_eq!(t.top + y, sp + row * ch, "misplaced at {sp}");
                    drawn.extend(row..row + rows);
                }
                drawn.sort_unstable();
                assert_eq!(drawn, (0..8).collect::<Vec<_>>(), "rows at {sp}, cell {ch}");
            }
        }
    }

    #[test]
    fn a_change_in_the_row_below_a_tile_bands_both_tiles() {
        let mut c = cache(true);
        upload(&mut c, 0, &[], 0);
        upload(&mut c, 1, &[0], 0);
        c.invalidate(100, 104);
        assert_eq!(c.plan_bands(&[0, 1], false, 0, 10, &mut String::new()), vec![(0, 100, 10), (1, 100, 10)]);
    }

    #[test]
    fn a_band_covers_every_row_it_touches_between_cells() {
        let mut c = cache(true);
        upload(&mut c, 1, &[], 100);
        c.invalidate(130, 135);
        let todo = c.plan_bands(&[1], false, 0, 10, &mut String::new());
        assert_eq!(todo, vec![(1, 130, 10)]);
        let mut b = Batch { keep: &[1], scroll_px: 100, snap: &mut snap, bytes: Vec::new(), cmds: String::new() };
        c.upload_band(1, 130, 10, &mut b);
        let v = view(104);
        let mut out = String::new();
        c.place(&c.visible(&v), &v, &mut out);
        let band = placements(&out).into_iter().find(|p| p.1 == 2).unwrap();
        assert_eq!(band, (2, 2, 4, 20, 2), "rows 2 and 3 show 124..144 from the image at 120..150");
    }

    #[test]
    fn bands_join_what_they_touch() {
        assert_eq!(join_bands(&[(38, 76), (76, 114)], &[(0, 76)]), (vec![(0, 114)], vec![0]));
        assert_eq!(join_bands(&[(380, 418)], &[(0, 76)]), (vec![(380, 418)], vec![]));
        assert_eq!(join_bands(&[(70, 160)], &[(0, 76), (152, 190), (300, 338)]), (vec![(0, 190)], vec![0, 1]));
        assert_eq!(join_bands(&[], &[(0, 76)]), (vec![], vec![]));
    }

    #[test]
    fn prefetch_picks_nearest_missing() {
        assert_eq!(prefetch_pick(&[(0, true, 10), (100, false, 10), (200, false, 10)], 190, 0, 100), Some(2));
    }

    #[test]
    fn prefetch_none_when_nearest_does_not_fit() {
        assert_eq!(prefetch_pick(&[(0, true, 10), (100, false, 10), (200, false, 10)], 190, 95, 100), None);
    }

    #[test]
    fn prefetch_none_when_all_resident() {
        assert_eq!(prefetch_pick(&[(0, true, 10), (100, true, 10)], 0, 20, 100), None);
    }
}
