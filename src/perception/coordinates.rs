// perception/coordinates.rs — pixel/screen coordinate contracts.
// All values are physical pixels (the process is per-monitor DPI aware);
// image coordinates are relative to the captured frame, screen coordinates are
// Windows virtual-desktop coordinates (negative origins are valid).

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w: w.max(0.0), h: h.max(0.0) }
    }
    pub fn from_i32(r: [i32; 4]) -> Self {
        Self::new(r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32)
    }
    pub fn x2(&self) -> f32 {
        self.x + self.w
    }
    pub fn y2(&self) -> f32 {
        self.y + self.h
    }
    pub fn area(&self) -> f32 {
        self.w * self.h
    }
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }
    pub fn intersection(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let x2 = self.x2().min(o.x2());
        let y2 = self.y2().min(o.y2());
        Rect::new(x, y, (x2 - x).max(0.0), (y2 - y).max(0.0))
    }
    pub fn inter_area(&self, o: &Rect) -> f32 {
        self.intersection(o).area()
    }
    pub fn iou(&self, o: &Rect) -> f32 {
        let i = self.inter_area(o);
        let u = self.area() + o.area() - i;
        if u <= 0.0 { 0.0 } else { i / u }
    }
    /// max(IoU, inter/A, inter/B) — the overlap measure used by OmniParser's
    /// remove_overlap_new, not ordinary NMS.
    pub fn overlap(&self, o: &Rect) -> f32 {
        let i = self.inter_area(o);
        let a = self.area();
        let b = o.area();
        let u = a + b - i;
        let iou = if u > 0.0 { i / u } else { 0.0 };
        let ra = if a > 0.0 { i / a } else { 0.0 };
        let rb = if b > 0.0 { i / b } else { 0.0 };
        iou.max(ra).max(rb)
    }
    pub fn contains_ratio(&self, inner: &Rect) -> f32 {
        let a = inner.area();
        if a <= 0.0 { 0.0 } else { self.inter_area(inner) / a }
    }
    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        let x2 = self.x2().max(o.x2());
        let y2 = self.y2().max(o.y2());
        Rect::new(x, y, x2 - x, y2 - y)
    }
    pub fn inset(&self, px: f32) -> Rect {
        Rect::new(self.x + px, self.y + px, (self.w - 2.0 * px).max(1.0), (self.h - 2.0 * px).max(1.0))
    }
    pub fn round_i32(&self) -> [i32; 4] {
        [
            self.x.round() as i32,
            self.y.round() as i32,
            self.w.round() as i32,
            self.h.round() as i32,
        ]
    }
    pub fn to_array(&self) -> [f32; 4] {
        [self.x, self.y, self.w, self.h]
    }
    pub fn clamp_to(&self, bound: &Rect) -> Rect {
        let i = self.intersection(bound);
        i
    }
}

/// Map an image-space rect to screen space. The frame's image is the window
/// bitmap (PrintWindow) or the virtual screen (BitBlt); the caller supplies the
/// image origin in screen coordinates. No implicit scaling is applied: if a
/// detector ran on a resized image, its boxes must first be mapped back with
/// `Letterbox::unmap_rect`.
#[derive(Clone, Copy, Debug)]
pub struct FrameMap {
    pub image_origin: [i32; 2],
    pub image_width: u32,
    pub image_height: u32,
}

impl FrameMap {
    pub fn to_screen(&self, r: &Rect) -> Rect {
        Rect::new(
            self.image_origin[0] as f32 + r.x,
            self.image_origin[1] as f32 + r.y,
            r.w,
            r.h,
        )
    }
    pub fn from_screen(&self, r: &Rect) -> Rect {
        Rect::new(
            r.x - self.image_origin[0] as f32,
            r.y - self.image_origin[1] as f32,
            r.w,
            r.h,
        )
    }
    pub fn bounds(&self) -> Rect {
        Rect::new(0.0, 0.0, self.image_width as f32, self.image_height as f32)
    }
}

/// Fixed-size letterbox used when a full frame is fed to a detector whose
/// ONNX input shape is static. The mapping is exact for the integer resize
/// and centered pad used by the exporter.
#[derive(Clone, Copy, Debug)]
pub struct Letterbox {
    pub src_w: u32,
    pub src_h: u32,
    pub dst_w: u32,
    pub dst_h: u32,
    pub scale: f32,
    pub pad_x: f32,
    pub pad_y: f32,
}

impl Letterbox {
    pub fn fit(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Self {
        let scale = (dst_w as f32 / src_w.max(1) as f32)
            .min(dst_h as f32 / src_h.max(1) as f32);
        let rw = (src_w as f32 * scale).round().max(1.0);
        let rh = (src_h as f32 * scale).round().max(1.0);
        Self {
            src_w,
            src_h,
            dst_w,
            dst_h,
            scale,
            pad_x: ((dst_w as f32 - rw) / 2.0).floor(),
            pad_y: ((dst_h as f32 - rh) / 2.0).floor(),
        }
    }
    pub fn map_rect(&self, r: &Rect) -> Rect {
        Rect::new(
            r.x * self.scale + self.pad_x,
            r.y * self.scale + self.pad_y,
            r.w * self.scale,
            r.h * self.scale,
        )
    }
    pub fn unmap_rect(&self, r: &Rect) -> Rect {
        let s = if self.scale <= 0.0 { 1.0 } else { self.scale };
        Rect::new(
            (r.x - self.pad_x) / s,
            (r.y - self.pad_y) / s,
            r.w / s,
            r.h / s,
        )
    }
    pub fn content_rect(&self) -> Rect {
        Rect::new(
            self.pad_x,
            self.pad_y,
            self.src_w as f32 * self.scale,
            self.src_h as f32 * self.scale,
        )
    }
}

/// Crop a rect to the frame; returns None for an empty crop.
pub fn crop_rect(frame_w: u32, frame_h: u32, r: &Rect) -> Option<Rect> {
    let c = r.clamp_to(&Rect::new(0.0, 0.0, frame_w as f32, frame_h as f32));
    if c.w < 1.0 || c.h < 1.0 { None } else { Some(c) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_matches_omniparser_measure() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let b = Rect::new(50.0, 0.0, 100.0, 100.0);
        // IoU = 1/3, inter/A = 0.5, inter/B = 0.5
        assert!((a.overlap(&b) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn letterbox_round_trip() {
        let lb = Letterbox::fit(1920, 1080, 640, 640);
        let r = Rect::new(100.0, 200.0, 30.0, 40.0);
        let back = lb.unmap_rect(&lb.map_rect(&r));
        assert!((back.x - r.x).abs() < 1e-3);
        assert!((back.y - r.y).abs() < 1e-3);
    }

    #[test]
    fn negative_screen_origin_is_preserved() {
        let m = FrameMap { image_origin: [-1920, -200], image_width: 800, image_height: 600 };
        let s = m.to_screen(&Rect::new(10.0, 20.0, 5.0, 6.0));
        assert_eq!(s.x, -1910.0);
        assert_eq!(s.y, -180.0);
    }
}
