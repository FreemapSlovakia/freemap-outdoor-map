use cairo::{Path, PathSegment};

pub fn draw_markers_on_path<F>(
    path: &Path,
    offset: f64,
    spacing: f64,
    draw_maker: &F,
) -> cairo::Result<()>
where
    F: Fn(f64, f64, f64) -> cairo::Result<()>,
{
    let mut m = offset;
    let mut px = 0.0;
    let mut py = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;

    let mut draw_on_line = |x: f64, y: f64, px: &mut f64, py: &mut f64| -> cairo::Result<()> {
        let d = (*px - x).hypot(*py - y);

        let mut off = spacing - m;

        m += d;

        while m >= spacing {
            let t = off / d;
            let xx = t.mul_add(x - *px, *px);
            let yy = t.mul_add(y - *py, *py);

            let angle = (y - *py).atan2(x - *px);

            // context.move_to(xx, yy);
            // context.arc(xx, yy, 3.0, 0.0, 6.2);
            // context.set_source_rgb(1.0, 0.0, 0.0);
            // context.fill()?;

            draw_maker(xx, yy, angle)?;

            m -= spacing;
            off += spacing;
        }

        *px = x;
        *py = y;

        Ok(())
    };

    for ps in path.iter() {
        match ps {
            PathSegment::MoveTo((x, y)) => {
                px = x;
                py = y;
                sx = x;
                sy = y;
            }
            PathSegment::LineTo((x, y)) => {
                draw_on_line(x, y, &mut px, &mut py)?;
            }
            PathSegment::ClosePath => {
                draw_on_line(sx, sy, &mut px, &mut py)?;
            }
            _ => panic!("unsupported path segment type: {ps:?}"),
        }
    }

    Ok(())
}
