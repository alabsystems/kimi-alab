// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

use super::*;

#[test]
fn a_ladder_clamps_to_its_last_step() {
    let s = |n| Duration::from_secs(n);
    let table = [s(1), s(5), s(15)];
    let l = Ladder(&table);
    assert_eq!(
        (0..6).map(|n| l.step(n)).collect::<Vec<_>>(),
        [s(1), s(5), s(15), s(15), s(15), s(15)]
    );
    assert_eq!(l.at(usize::MAX), Some(s(15)));
    assert_eq!(Ladder(&[]).at(0), None);
}

#[test]
fn a_doubling_back_off_doubles_to_its_cap() {
    let ms = Duration::from_millis;
    assert_eq!(
        (0..6)
            .map(|n| doubling(ms(500), n, ms(6000)))
            .collect::<Vec<_>>(),
        [ms(500), ms(1000), ms(2000), ms(4000), ms(6000), ms(6000)]
    );
    assert_eq!(doubling(ms(500), 31, ms(6000)), ms(6000));
    assert_eq!(doubling(ms(500), u32::MAX, ms(6000)), ms(6000));
    assert_eq!(doubling(Duration::MAX, 1, ms(6000)), ms(6000));
    assert_eq!(doubling(ms(500), 0, ms(100)), ms(100));
}
