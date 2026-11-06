//! グリフのビットマップへの ID の表。glyphon の `CustomGlyph::id` は `u16` なので、振れる数に上限がある。

use std::collections::HashMap;

use cosmic_text::{CacheKey, SwashContent, SwashImage};
use glyphon::ContentType;

/// `u16` の ID で振れるグリフの数。
pub(crate) const MAX_GLYPH_IDS: usize = u16::MAX as usize + 1;

/// 取り出したグリフのビットマップ。atlas に載せる元のデータ。
pub(crate) struct GlyphBitmap {
    pub(crate) content: ContentType,
    pub(crate) data: Vec<u8>,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

/// 振れる ID を使い切った。呼び出し側が表と atlas を作り直して振り直す。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct IdsExhausted;

/// グリフの `CacheKey` から ID を引く表と、ID からビットマップを引く表。
pub(crate) struct GlyphIds {
    limit: usize,
    ids: HashMap<CacheKey, u16>,
    bitmaps: Vec<GlyphBitmap>,
}

impl GlyphIds {
    /// 振る ID の数の上限を指定して作る。
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self {
            limit: limit.min(MAX_GLYPH_IDS),
            ids: HashMap::new(),
            bitmaps: Vec::new(),
        }
    }

    /// 振り済みの数。
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.bitmaps.len()
    }

    /// ID のビットマップ。
    pub(crate) fn bitmap(&self, id: u16) -> Option<&GlyphBitmap> {
        self.bitmaps.get(usize::from(id))
    }

    /// グリフのビットマップに ID を振る。同じ `CacheKey` は同じ ID（atlas の再利用のキー）。
    /// ビットマップが空や対応外なら `Ok(None)`。上限を超えるなら、何も振らずに `Err`。
    pub(crate) fn intern(
        &mut self,
        key: CacheKey,
        image: &SwashImage,
    ) -> Result<Option<(u16, u16, u16)>, IdsExhausted> {
        let content = match image.content {
            SwashContent::Mask => ContentType::Mask,
            SwashContent::Color => ContentType::Color,
            SwashContent::SubpixelMask => return Ok(None),
        };
        let (w, h) = (image.placement.width as u16, image.placement.height as u16);
        if w == 0 || h == 0 {
            return Ok(None);
        }
        if let Some(&id) = self.ids.get(&key) {
            return Ok(Some((id, w, h)));
        }
        let id = u16::try_from(self.bitmaps.len()).map_err(|_| IdsExhausted)?;
        if self.bitmaps.len() >= self.limit {
            return Err(IdsExhausted);
        }
        self.bitmaps.push(GlyphBitmap {
            content,
            data: image.data.clone(),
            width: w,
            height: h,
        });
        self.ids.insert(key, id);
        Ok(Some((id, w, h)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_text::{fontdb, CacheKeyFlags};

    fn key(glyph_id: u16) -> CacheKey {
        CacheKey::new(
            fontdb::ID::dummy(),
            glyph_id,
            16.0,
            (0.0, 0.0),
            fontdb::Weight::NORMAL,
            CacheKeyFlags::empty(),
        )
        .0
    }

    fn image() -> SwashImage {
        let mut image = SwashImage::new();
        image.content = SwashContent::Mask;
        image.placement.width = 2;
        image.placement.height = 2;
        image.data = vec![255; 4];
        image
    }

    #[test]
    fn 同じグリフは同じ_id_で別のグリフは別の_id() {
        let mut ids = GlyphIds::with_limit(MAX_GLYPH_IDS);
        let a = ids.intern(key(1), &image()).unwrap().unwrap().0;
        let b = ids.intern(key(2), &image()).unwrap().unwrap().0;
        assert_ne!(a, b);
        assert_eq!(ids.intern(key(1), &image()).unwrap().unwrap().0, a);
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn 上限を超えるグリフは折り返さずに使い切りを返す() {
        let mut ids = GlyphIds::with_limit(3);
        for g in 0..3 {
            assert!(ids.intern(key(g), &image()).unwrap().is_some());
        }
        assert_eq!(ids.intern(key(3), &image()), Err(IdsExhausted));
        assert_eq!(ids.len(), 3, "使い切りでは何も振らない");
        assert!(
            ids.intern(key(0), &image()).is_ok(),
            "振り済みのグリフは引ける"
        );
    }

    #[test]
    fn u16_の全域を重なりなく振る() {
        let mut ids = GlyphIds::with_limit(MAX_GLYPH_IDS);
        let mut seen = std::collections::HashSet::new();
        for g in 0..=u16::MAX {
            let (id, _, _) = ids.intern(key(g), &image()).unwrap().unwrap();
            assert!(seen.insert(id), "ID {id} が重なった");
        }
        assert_eq!(seen.len(), MAX_GLYPH_IDS);
        let next = CacheKey::new(
            fontdb::ID::dummy(),
            0,
            17.0,
            (0.0, 0.0),
            fontdb::Weight::NORMAL,
            CacheKeyFlags::empty(),
        )
        .0;
        assert_eq!(ids.intern(next, &image()), Err(IdsExhausted));
    }
}
