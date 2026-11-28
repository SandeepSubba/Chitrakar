//! Tiling: the invalidation granularity of the cached render graph. Editing
//! one object re-renders only the tiles its bounds touch.

pub const TILE_SIZE: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCoord {
    pub col: u32,
    pub row: u32,
}

/// Tile grid covering a surface of the given pixel size.
#[derive(Debug, Clone, Copy)]
pub struct TileGrid {
    pub cols: u32,
    pub rows: u32,
}

impl TileGrid {
    pub fn covering(width: u32, height: u32) -> Self {
        Self {
            cols: width.div_ceil(TILE_SIZE),
            rows: height.div_ceil(TILE_SIZE),
        }
    }

    pub fn tile_count(&self) -> u32 {
        self.cols * self.rows
    }

    /// Tiles intersecting a pixel-space rectangle (min inclusive, max
    /// exclusive), clamped to the grid.
    pub fn tiles_in_rect(
        &self,
        min_x: u32,
        min_y: u32,
        max_x: u32,
        max_y: u32,
    ) -> impl Iterator<Item = TileCoord> + '_ {
        let c0 = (min_x / TILE_SIZE).min(self.cols);
        let r0 = (min_y / TILE_SIZE).min(self.rows);
        let c1 = max_x.div_ceil(TILE_SIZE).min(self.cols);
        let r1 = max_y.div_ceil(TILE_SIZE).min(self.rows);
        (r0..r1).flat_map(move |row| (c0..c1).map(move |col| TileCoord { col, row }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_covers_partial_tiles() {
        let g = TileGrid::covering(1000, 300);
        assert_eq!((g.cols, g.rows), (4, 2));
        assert_eq!(g.tile_count(), 8);
    }

    #[test]
    fn dirty_rect_maps_to_touched_tiles() {
        let g = TileGrid::covering(1024, 1024);
        let tiles: Vec<_> = g.tiles_in_rect(250, 0, 300, 10).collect();
        // Rect spans the tile-0/tile-1 boundary at x=256.
        assert_eq!(
            tiles,
            vec![TileCoord { col: 0, row: 0 }, TileCoord { col: 1, row: 0 }]
        );
    }

    #[test]
    fn rect_outside_grid_is_clamped() {
        let g = TileGrid::covering(256, 256);
        assert_eq!(g.tiles_in_rect(5000, 5000, 6000, 6000).count(), 0);
    }
}
