//! The next number in a counter. One place owns that step.

pub(crate) fn take_next<T>(slot: &mut T) -> T
where
    T: Copy + std::ops::AddAssign + From<u8>,
{
    let current = *slot;
    *slot += T::from(1);
    current
}
