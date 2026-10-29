use yomydev_core::{CameraFault, Detail, Luma, Sight, View};

const STEP: usize = 4;
const COLUMNS: usize = 16;
const ROWS: usize = 12;
const INTERIOR: usize = (COLUMNS - 2) * (ROWS - 2);
const EDGE: u32 = 12;
const STENCIL: usize = 3;
const NEIGHBOURS: u32 = 4;

type Grid = [[Luma; COLUMNS]; ROWS];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) person: bool,
    pub(crate) luma: Luma,
    pub(crate) detail: Detail,
}

pub(crate) fn measure(
    plane: &[u8],
    width: usize,
    height: usize,
    stride: usize,
) -> Option<(Luma, Detail)> {
    let mut sums = [[0_u64; COLUMNS]; ROWS];
    let mut counts = [[0_u64; COLUMNS]; ROWS];
    for row in (0..height).step_by(STEP) {
        let start = row.checked_mul(stride)?;
        let line = plane.get(start..start.checked_add(width)?)?;
        let cell_row = row.checked_mul(ROWS)?.checked_div(height)?;
        for (column, &value) in line.iter().enumerate().step_by(STEP) {
            let cell_column = column.checked_mul(COLUMNS)?.checked_div(width)?;
            let sum = sums.get_mut(cell_row)?.get_mut(cell_column)?;
            *sum = sum.checked_add(u64::from(value))?;
            let count = counts.get_mut(cell_row)?.get_mut(cell_column)?;
            *count = count.checked_add(1)?;
        }
    }
    let mut grid: Grid = [[Luma::new(0); COLUMNS]; ROWS];
    for ((means, row_sums), row_counts) in grid.iter_mut().zip(&sums).zip(&counts) {
        for ((mean, &sum), &count) in means.iter_mut().zip(row_sums).zip(row_counts) {
            *mean = average(sum, count)?;
        }
    }
    let total = sums.iter().flatten().sum::<u64>();
    let count = counts.iter().flatten().sum::<u64>();
    Some((average(total, count)?, detail(&grid)?))
}

fn average(sum: u64, count: u64) -> Option<Luma> {
    match u8::try_from(sum.checked_div(count)?) {
        Ok(mean) => Some(Luma::new(mean)),
        Err(_) => None,
    }
}

fn detail(grid: &Grid) -> Option<Detail> {
    let mut edged = 0_usize;
    for rows in grid.windows(STENCIL) {
        let [above, here, below] = rows else {
            return None;
        };
        let vertical = above.iter().zip(below).skip(1);
        for (cells, (&up, &down)) in here.windows(STENCIL).zip(vertical) {
            let &[left, centre, right] = cells else {
                return None;
            };
            let around = [left, right, up, down]
                .map(|cell| u32::from(cell.get()))
                .iter()
                .sum::<u32>();
            if u32::from(centre.get())
                .checked_mul(NEIGHBOURS)?
                .abs_diff(around)
                >= EDGE
            {
                edged = edged.checked_add(1)?;
            }
        }
    }
    match u8::try_from(
        edged
            .checked_mul(usize::from(Detail::WHOLE))?
            .checked_div(INTERIOR)?,
    ) {
        Ok(percent) => Detail::new(percent),
        Err(_) => None,
    }
}

pub(crate) fn summarize(frames: &[Option<Frame>]) -> Sight {
    let analysed = || frames.iter().flatten();
    if analysed().any(|frame| frame.person) {
        return Sight::Person;
    }
    let brightest = analysed().map(|frame| frame.luma).max();
    let flattest = analysed().map(|frame| frame.detail).min();
    match (brightest, flattest) {
        _ if frames.is_empty() => Sight::Unavailable(CameraFault::TimedOut),
        (Some(brightest), Some(flattest)) if frames.iter().all(Option::is_some) => {
            Sight::Nobody(View {
                brightest,
                flattest,
            })
        },
        _ => Sight::Unavailable(CameraFault::Failed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: usize = 128;
    const HEIGHT: usize = 96;
    const STRIDE: usize = WIDTH + 16;

    fn plane(pixel: impl Fn(usize) -> u8) -> Vec<u8> {
        let mut plane = vec![0_u8; STRIDE * HEIGHT];
        for line in plane.chunks_mut(STRIDE) {
            for (column, value) in line.iter_mut().take(WIDTH).enumerate() {
                *value = pixel(column);
            }
        }
        plane
    }

    fn detail(percent: u8) -> Detail {
        Detail::new(percent).unwrap()
    }

    fn frame(person: bool, luma: u8, percent: u8) -> Frame {
        Frame {
            person,
            luma: Luma::new(luma),
            detail: detail(percent),
        }
    }

    #[test]
    fn a_uniform_bright_plane_has_no_detail() {
        let measured = measure(&plane(|_| 200), WIDTH, HEIGHT, STRIDE);
        assert_eq!(measured, Some((Luma::new(200), detail(0))));
    }

    #[test]
    fn a_smooth_gradient_has_no_detail() {
        let ramp = |column: usize| u8::try_from(column * 2).unwrap();
        let measured = measure(&plane(ramp), WIDTH, HEIGHT, STRIDE);
        assert_eq!(measured.map(|(_, detail)| detail), Some(detail(0)));
    }

    #[test]
    fn an_edge_marks_the_cells_on_both_sides() {
        let step = |column: usize| if column < WIDTH / 2 { 40 } else { 200 };
        let measured = measure(&plane(step), WIDTH, HEIGHT, STRIDE);
        assert_eq!(measured, Some((Luma::new(120), detail(14))));
    }

    #[test]
    fn measure_refuses_an_empty_short_or_tiny_plane() {
        assert_eq!(measure(&[], 0, 0, 0), None);
        assert_eq!(measure(&[1, 2, 3], 4, 1, 4), None);
        assert_eq!(measure(&[0; 8], 4, 9, usize::MAX), None);
        assert_eq!(measure(&[0; 64 * 8], 64, 8, 64), None);
    }

    #[test]
    fn a_person_in_any_frame_wins() {
        assert_eq!(
            summarize(&[None, Some(frame(false, 90, 0)), Some(frame(true, 20, 0))]),
            Sight::Person
        );
    }

    #[test]
    fn nobody_keeps_the_brightest_luma_and_the_flattest_detail() {
        assert_eq!(
            summarize(&[Some(frame(false, 30, 2)), Some(frame(false, 45, 20))]),
            Sight::Nobody(View {
                brightest: Luma::new(45),
                flattest: detail(2),
            })
        );
    }

    #[test]
    fn nobody_needs_every_frame_analysed() {
        assert_eq!(
            summarize(&[Some(frame(false, 30, 9)), None]),
            Sight::Unavailable(CameraFault::Failed)
        );
        assert_eq!(summarize(&[None]), Sight::Unavailable(CameraFault::Failed));
    }

    #[test]
    fn no_frames_means_the_camera_timed_out() {
        assert_eq!(summarize(&[]), Sight::Unavailable(CameraFault::TimedOut));
    }
}
